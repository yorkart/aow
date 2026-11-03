use bytes::Bytes;
use http::StatusCode;
use serde::de::DeserializeOwned;

use super::{LocalClientError, error::status_error};

pub(super) fn expect_json<T: DeserializeOwned>(
    status: StatusCode,
    body: Bytes,
    expected: &[StatusCode],
) -> Result<T, LocalClientError> {
    if !expected.contains(&status) {
        return Err(status_error(status, &body));
    }
    Ok(serde_json::from_slice(&body)?)
}

pub(super) fn expect_empty(
    status: StatusCode,
    body: Bytes,
    expected: &[StatusCode],
) -> Result<(), LocalClientError> {
    if expected.contains(&status) {
        Ok(())
    } else {
        Err(status_error(status, &body))
    }
}
