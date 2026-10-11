//! Preserve ACP authentication state across the HTTP host boundary.
pub fn is_auth_required(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<agent_client_protocol::Error>()
        .is_some_and(|error| error.code == agent_client_protocol::ErrorCode::AuthRequired)
}

#[cfg(test)]
mod tests {
    #[test]
    fn authentication_is_classified_by_protocol_code_not_message() {
        let auth = anyhow::Error::new(agent_client_protocol::Error::auth_required())
            .context("Opening session");
        assert!(super::is_auth_required(&auth));
        assert!(!super::is_auth_required(&anyhow::anyhow!(
            "Authentication required"
        )));
    }
}
