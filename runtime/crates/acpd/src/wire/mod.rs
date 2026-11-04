//! Native ACP transport. Frames contain ACP JSON-RPC directly, never facade DTOs.

mod http;
mod instance;

use anyhow::{Context, Result, ensure};
use aow_acp::{AcpService, ValidateExecution};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, watch};

use instance::Instance;

#[derive(Clone)]
pub(crate) struct WireRuntime {
    service: AcpService,
    host: Arc<crate::host::Host>,
    settings_lock: Arc<Mutex<()>>,
    instances: Arc<Mutex<BTreeMap<String, Arc<Instance>>>>,
    stopping: watch::Receiver<bool>,
}

use aow_runtime_contract::acp::{AcpInstance as InstanceInfo, AcpInstanceSpec as InstanceSpec};

impl WireRuntime {
    pub fn new(
        service: AcpService,
        host: Arc<crate::host::Host>,
        stopping: watch::Receiver<bool>,
    ) -> Self {
        Self {
            service,
            host,
            settings_lock: Default::default(),
            instances: Default::default(),
            stopping,
        }
    }

    pub fn router(&self) -> axum::Router {
        http::router(self.clone())
    }

    async fn create(
        &self,
        id: String,
        spec: InstanceSpec,
    ) -> Result<(Arc<Instance>, bool), ApiError> {
        ensure_id(&id)?;
        // Serialize admission with shutdown. The stop watch also cancels slow Registry installs.
        let mut instances = self.instances.lock().await;
        if *self.stopping.borrow() {
            return Err(ApiError::stopping());
        }
        if let Some(instance) = instances.get(&id) {
            if instance.spec != spec {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "Instance ID already has another specification".into(),
                ));
            }
            return Ok((instance.clone(), false));
        }
        if !spec.cwd.is_absolute() || !spec.cwd.is_dir() {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "cwd must be an existing absolute directory on this node".into(),
            ));
        }
        if instances.len() >= 256 {
            return Err(ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "ACP instance limit reached; delete unused instances".into(),
            ));
        }
        if spec.environment.len() > 128
            || spec.environment.iter().any(|(key, value)| {
                key.is_empty() || key.contains(['=', '\0']) || value.contains('\0')
            })
            || spec
                .environment
                .iter()
                .map(|(key, value)| key.len() + value.len())
                .sum::<usize>()
                > 128 * 1024
        {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "Invalid instance environment".into(),
            ));
        }
        if let Some(environment) = &spec.execution_environment {
            environment.validate().map_err(ApiError::invalid)?;
        }
        if self.host.filesystem.read().await.contains(&spec.cwd) {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "Workspace is being removed".into(),
            ));
        }
        let mut stop = self.stopping.clone();
        let command = tokio::select! {
            result = self.service.prepare_adapter(&spec.agent_id) => result.map_err(ApiError::invalid)?,
            _ = stop.changed() => return Err(ApiError::stopping()),
        };
        if *stop.borrow() {
            return Err(ApiError::stopping());
        }
        if self.host.filesystem.read().await.contains(&spec.cwd) {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "Workspace is being removed".into(),
            ));
        }
        let instance = Arc::new(
            Instance::spawn(id.clone(), spec, command, self.stopping.clone())
                .map_err(ApiError::invalid)?,
        );
        instances.insert(id, instance.clone());
        Ok((instance, true))
    }

    async fn get(&self, id: &str) -> Result<Arc<Instance>, ApiError> {
        self.instances
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "ACP instance not found".into()))
    }

    async fn delete(&self, id: &str) {
        // Retain the handle until reap completes, even if the HTTP caller disappears.
        let mut instances = self.instances.lock().await;
        if let Some(instance) = instances.get(id) {
            instance.stop().await;
        }
        instances.remove(id);
    }

    pub async fn shutdown(&self) {
        let mut instances = self.instances.lock().await;
        for instance in instances.values() {
            instance.request_stop();
        }
        for instance in instances.values() {
            instance.stop().await;
        }
        instances.clear();
    }
}

fn ensure_id(id: &str) -> Result<(), ApiError> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid ACP instance ID".into(),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct ApiError(StatusCode, String);
impl ApiError {
    fn invalid(error: impl std::fmt::Display) -> Self {
        Self(StatusCode::BAD_REQUEST, error.to_string())
    }
    fn stopping() -> Self {
        Self(
            StatusCode::SERVICE_UNAVAILABLE,
            "ACP runtime is shutting down".into(),
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error": self.1}))).into_response()
    }
}
