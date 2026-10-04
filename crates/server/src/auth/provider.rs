use serde_json::Value;

use crate::HttpError;

/// Providers return an identity, never a browser token. `revision` is opaque
/// to the shared session store and changes when this identity is revoked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct VerifiedIdentity {
    pub subject: String,
    pub revision: String,
}

/// A login method owns credential verification and its revocation rules.
/// Sessions, cookies, and access policy are handled by the shared auth module.
pub(super) trait LoginProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn check_configured(&self) -> Result<(), HttpError>;
    fn authenticate(&self, credentials: &Value) -> Result<VerifiedIdentity, HttpError>;
    fn validate_session(&self, identity: &VerifiedIdentity) -> Result<bool, HttpError>;
}
