//! Durable node environment transactions. Server publishes the business decision.
use anyhow::{Context, Result, ensure};
use aow_host_contract::environment::{EnvironmentPreparation, PreparedEnvironment};
use aow_host_environment::{EnvironmentManager, PreparedHome, RoleEnvironmentSpec};
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub(crate) struct Environments(Arc<Inner>);
struct Inner {
    prober: aow_acp_runtime_service::AcpProber,
    manager: EnvironmentManager,
    root: PathBuf,
    pending: Mutex<HashMap<String, Pending>>,
}
#[derive(Serialize, Deserialize)]
struct Pending {
    spec: RoleEnvironmentSpec,
    journal: serde_json::Value,
}
#[derive(Serialize, Deserialize)]
struct Decision {
    spec: RoleEnvironmentSpec,
    commit: bool,
}
use aow_host_contract::environment::{
    FinishEnvironment as Finish, PrepareEnvironment as Prepare, ProbeEnvironment as Probe,
};
type Response = Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>;
fn error(error: anyhow::Error) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({"message":error.to_string()})),
    )
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("Invalid environment metadata path")?;
    std::fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(value)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
impl Environments {
    pub(crate) fn open(root: PathBuf, prober: aow_acp_runtime_service::AcpProber) -> Result<Self> {
        let directory = root.join("data/environments/transactions");
        std::fs::create_dir_all(&directory)?;
        let service = Self(Arc::new(Inner {
            prober,
            manager: EnvironmentManager::new(root.clone())?,
            root,
            pending: Mutex::default(),
        }));
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let token = path.file_stem().unwrap().to_string_lossy().into_owned();
            uuid::Uuid::parse_str(&token)?;
            let pending: Pending = serde_json::from_slice(&std::fs::read(&path)?)?;
            service.0.manager.validate(&pending.spec)?;
            if let Some(decision) = service.decision(&token)? {
                ensure!(
                    decision.spec == pending.spec,
                    "Environment transaction decision changed identity"
                );
                let home = PreparedHome::restore(pending.journal)?;
                if decision.commit {
                    service.save_spec(&pending.spec)?;
                    home.commit();
                } else {
                    home.rollback()?;
                }
                std::fs::remove_file(path)?;
            } else {
                service.0.pending.lock().unwrap().insert(token, pending);
            }
        }
        Ok(service)
    }
    pub(crate) fn router(&self) -> Router {
        Router::new()
            .route("/v1/environments/roles/prepare", post(prepare))
            .route("/v1/environments/roles/finish", post(finish))
            .route("/v1/environments/roles/resolve", post(resolve))
            .route("/v1/environments/roles/probe", post(probe))
            .route("/v1/environments/identity-sources", post(sources))
            .with_state(self.clone())
    }
    fn transaction_path(&self, token: &str) -> Result<PathBuf> {
        uuid::Uuid::parse_str(token)?;
        Ok(self
            .0
            .root
            .join("data/environments/transactions")
            .join(format!("{token}.json")))
    }
    fn decision_path(&self, token: &str) -> Result<PathBuf> {
        uuid::Uuid::parse_str(token)?;
        Ok(self
            .0
            .root
            .join("data/environments/decisions")
            .join(format!("{token}.json")))
    }
    fn decision(&self, token: &str) -> Result<Option<Decision>> {
        match std::fs::read(self.decision_path(token)?) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    fn spec_path(&self, spec: &RoleEnvironmentSpec) -> Result<PathBuf> {
        self.0.manager.validate(spec)?;
        Ok(self
            .0
            .root
            .join("data/environments/roles")
            .join(&spec.id)
            .join("spec.json"))
    }
    fn previous(&self, spec: &RoleEnvironmentSpec) -> Result<Option<RoleEnvironmentSpec>> {
        match std::fs::read(self.spec_path(spec)?) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    fn save_spec(&self, spec: &RoleEnvironmentSpec) -> Result<()> {
        write_json(&self.spec_path(spec)?, spec)
    }
    fn environment(&self, spec: &RoleEnvironmentSpec) -> Result<PreparedEnvironment> {
        Ok(PreparedEnvironment {
            home: self.0.manager.role_home(&spec.id)?,
            env: self.0.manager.role_environment(spec)?,
        })
    }
    fn ensure(&self, spec: &RoleEnvironmentSpec) -> Result<PreparedEnvironment> {
        let pending = self.0.pending.lock().unwrap();
        if let Some(operation) = pending
            .values()
            .find(|operation| operation.spec.id == spec.id)
        {
            ensure!(
                operation.spec == *spec,
                "Another role environment update is in progress"
            );
            return self.environment(spec);
        }
        let previous = self.previous(spec)?;
        if previous.as_ref() != Some(spec) {
            let home = self.0.manager.prepare_role_home(spec, previous.as_ref())?;
            self.save_spec(spec)?;
            home.commit();
        }
        self.environment(spec)
    }
    fn decide_locked(
        &self,
        pending: &mut HashMap<String, Pending>,
        token: &str,
        spec: &RoleEnvironmentSpec,
        commit: bool,
    ) -> Result<()> {
        if let Some(decision) = self.decision(token)? {
            ensure!(
                decision.spec == *spec && decision.commit == commit,
                "Environment transaction decision is immutable"
            );
        }
        if let Some(operation) = pending.get(token) {
            ensure!(
                operation.spec == *spec,
                "Environment transaction identity changed"
            );
            if commit {
                self.save_spec(spec)?;
            }
            write_json(
                &self.decision_path(token)?,
                &Decision {
                    spec: spec.clone(),
                    commit,
                },
            )?;
            let home = PreparedHome::restore(operation.journal.clone())?;
            if commit {
                home.commit();
            } else {
                home.rollback()?;
            }
            std::fs::remove_file(self.transaction_path(token)?)?;
            pending.remove(token);
        } else if self.decision(token)?.is_none() {
            // Retain early cancellation and completed-decision idempotency across restart.
            write_json(
                &self.decision_path(token)?,
                &Decision {
                    spec: spec.clone(),
                    commit,
                },
            )?;
        }
        Ok(())
    }
}
async fn prepare(State(service): State<Environments>, Json(request): Json<Prepare>) -> Response {
    aow_i18n::spawn_blocking(move || {
        service.0.manager.validate(&request.spec)?;
        let token = request
            .token
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        service.transaction_path(&token)?;
        let mut pending = service.0.pending.lock().unwrap();
        if let Some(decision) = service.decision(&token)? {
            ensure!(
                decision.spec == request.spec && decision.commit,
                "Environment preparation was cancelled or changed"
            );
            let environment = service.environment(&request.spec)?;
            return Ok(Json(serde_json::to_value(EnvironmentPreparation {
                token,
                environment,
            })?));
        }
        if let Some(operation) = pending.get(&token) {
            ensure!(
                operation.spec == request.spec,
                "Environment transaction identity changed"
            );
            let environment = service.environment(&request.spec)?;
            return Ok(Json(serde_json::to_value(EnvironmentPreparation {
                token,
                environment,
            })?));
        }
        ensure!(pending.len() < 64, "Too many pending environment updates");
        ensure!(
            !pending
                .values()
                .any(|operation| operation.spec.id == request.spec.id),
            "Role environment update is in progress"
        );
        let previous = service.previous(&request.spec)?;
        let home = service
            .0
            .manager
            .prepare_role_home(&request.spec, previous.as_ref())?;
        let operation = Pending {
            spec: request.spec.clone(),
            journal: home.journal()?,
        };
        write_json(&service.transaction_path(&token)?, &operation)?;
        home.suspend();
        pending.insert(token.clone(), operation);
        let environment = service.environment(&request.spec)?;
        Ok(Json(serde_json::to_value(EnvironmentPreparation {
            token,
            environment,
        })?))
    })
    .await
    .map_err(|e| error(e.into()))?
    .map_err(error)
}
async fn finish(State(service): State<Environments>, Json(request): Json<Finish>) -> Response {
    aow_i18n::spawn_blocking(move || {
        service.0.manager.validate(&request.spec)?;
        {
            let mut pending = service.0.pending.lock().unwrap();
            service.decide_locked(&mut pending, &request.token, &request.spec, request.commit)?;
        }
        Ok(Json(serde_json::json!({})))
    })
    .await
    .map_err(|e| error(e.into()))?
    .map_err(error)
}
async fn resolve(State(service): State<Environments>, Json(request): Json<Prepare>) -> Response {
    aow_i18n::spawn_blocking(move || {
        service.0.manager.validate(&request.spec)?;
        if request.published {
            let mut pending = service.0.pending.lock().unwrap();
            if let Some((token, spec)) = pending
                .iter()
                .find(|(_, operation)| operation.spec.id == request.spec.id)
                .map(|(token, operation)| (token.clone(), operation.spec.clone()))
            {
                service.decide_locked(&mut pending, &token, &spec, spec == request.spec)?;
            }
        }
        service
            .ensure(&request.spec)
            .and_then(|environment| Ok(Json(serde_json::to_value(environment)?)))
    })
    .await
    .map_err(|e| error(e.into()))?
    .map_err(error)
}
async fn sources(State(service): State<Environments>) -> Response {
    Ok(Json(
        serde_json::to_value(service.0.manager.identity_sources())
            .context("Identity sources")
            .map_err(error)?,
    ))
}
async fn probe(State(service): State<Environments>, Json(request): Json<Probe>) -> Response {
    if request.timeout_ms == 0 || request.timeout_ms > 120_000 {
        return Err(error(anyhow::anyhow!("Invalid probe timeout")));
    }
    tokio::time::timeout(
        std::time::Duration::from_millis(request.timeout_ms),
        async {
            let environment = service.ensure(&request.spec)?;
            let home = environment.home;
            let workspace = tempfile::Builder::new()
                .prefix("connection-")
                .tempdir_in(home.parent().unwrap())?;
            let result =
                aow_workspaces::command(workspace.path(), &["init".into(), "--quiet".into()])
                    .await?;
            ensure!(
                result.success,
                "Unable to initialize connection test workspace"
            );
            let mut env = service.0.manager.role_environment(&request.spec)?;
            if let Some(mut launch) = request.launch {
                launch.env.extend(env);
                aow_agent_runtime_core::management::probe::cli(
                    launch,
                    workspace.path(),
                    request.timeout_ms,
                )
                .await?;
            } else {
                let agent = request.acp_agent_id.context("Missing probe runtime")?;
                env.remove("HOME");
                env.remove("AOW_AGENT_ROLE_ID");
                service
                    .0
                    .prober
                    .probe(
                        &agent,
                        workspace.path().into(),
                        aow_acp_runtime_core::ExecutionEnvironment { home, env },
                    )
                    .await?;
            }
            Ok(Json(serde_json::json!({})))
        },
    )
    .await
    .map_err(|_| error(anyhow::anyhow!("Connection test timed out")))?
    .map_err(error)
}
