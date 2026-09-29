use super::store::{conflict, identifier};
use crate::HttpError;
use serde::{Deserialize, Serialize};

/// Persisted with the resource, so a lost response or restart can be retried.
/// The key only identifies the request; it is never a resource ID or filename.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct CreationRequest {
    pub key: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct CreationReceipt {
    pub id: String,
    pub fingerprint: String,
}

impl CreationRequest {
    pub fn new(key: &str, input: &impl Serialize) -> Result<Self, HttpError> {
        identifier(key)?;
        let bytes = serde_json::to_vec(input).map_err(|e| HttpError::internal(e.to_string()))?;
        let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
        Ok(Self {
            key: key.into(),
            fingerprint: digest
                .as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        })
    }

    pub fn verify(&self, fingerprint: &str) -> Result<(), HttpError> {
        if self.fingerprint != fingerprint {
            return Err(conflict(
                "Request key was already used with different content",
            ));
        }
        Ok(())
    }

    pub fn receipt(&self, id: &str) -> CreationReceipt {
        CreationReceipt {
            id: id.into(),
            fingerprint: self.fingerprint.clone(),
        }
    }
}
