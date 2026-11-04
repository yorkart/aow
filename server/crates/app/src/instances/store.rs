use crate::HttpError;
use aow_protocol::{InstanceInfo, InstanceRef};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub(crate) struct InstanceStore(Arc<Inner>);

#[derive(Default)]
struct Inner {
    path: Option<PathBuf>,
    records: Mutex<BTreeMap<InstanceRef, InstanceInfo>>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    items: Vec<InstanceInfo>,
}

impl InstanceStore {
    pub(crate) fn persistent(directory: &Path) -> anyhow::Result<Self> {
        let path = directory.join("state/instances.json");
        let mut records = BTreeMap::new();
        match std::fs::read(&path) {
            Ok(bytes) => {
                let saved: Saved = serde_json::from_slice(&bytes)?;
                anyhow::ensure!(saved.version == 1, "unsupported instance history version");
                for item in saved.items {
                    item.instance.validate().map_err(anyhow::Error::msg)?;
                    if let Some(parent) = &item.parent {
                        parent.validate().map_err(anyhow::Error::msg)?;
                    }
                    anyhow::ensure!(
                        records.insert(item.instance.clone(), item).is_none(),
                        "duplicate instance history ID"
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(Self(Arc::new(Inner {
            path: Some(path),
            records: Mutex::new(records),
        })))
    }

    pub(crate) fn snapshot(&self) -> Result<BTreeMap<InstanceRef, InstanceInfo>, HttpError> {
        Ok(self
            .0
            .records
            .lock()
            .map_err(|e| HttpError::internal(e.to_string()))?
            .clone())
    }

    pub(crate) fn record_all(&self, items: Vec<InstanceInfo>) -> Result<(), HttpError> {
        let mut records = self
            .0
            .records
            .lock()
            .map_err(|e| HttpError::internal(e.to_string()))?;
        let mut next = records.clone();
        for item in items {
            if next
                .get(&item.instance)
                .is_some_and(|previous| previous.parent != item.parent)
            {
                return Err(HttpError::internal(aow_i18n::text(
                    "validation.instance_creation_parent_cannot_change",
                )));
            }
            next.insert(item.instance.clone(), item);
        }
        if let Some(path) = &self.0.path {
            let saved = Saved {
                version: 1,
                items: next.values().cloned().collect(),
            };
            let bytes =
                serde_json::to_vec(&saved).map_err(|e| HttpError::internal(e.to_string()))?;
            aow_automations::store::atomic_write(path, &bytes)
                .map_err(|e| HttpError::internal(e.to_string()))?;
        }
        *records = next;
        Ok(())
    }
}
