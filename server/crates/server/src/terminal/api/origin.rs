use super::*;

pub(crate) fn validate_request_origin(headers: &HeaderMap) -> Result<(), TerminalError> {
    let Some(origin) = headers.get(ORIGIN) else {
        return Ok(());
    };
    let origin = origin
        .to_str()
        .map_err(|_| TerminalError::ForbiddenOrigin("Origin is not valid ASCII".to_owned()))?;
    let origin: Uri = origin
        .parse()
        .map_err(|_| TerminalError::ForbiddenOrigin("Origin is not a valid URL".to_owned()))?;
    if !matches!(origin.scheme_str(), Some("http" | "https"))
        || origin.path() != "/"
        || origin.query().is_some()
    {
        return Err(TerminalError::ForbiddenOrigin(
            "Origin must be an HTTP origin without a path or query".to_owned(),
        ));
    }
    let origin_authority = origin.authority().ok_or_else(|| {
        TerminalError::ForbiddenOrigin("Origin does not contain a host".to_owned())
    })?;
    let host = headers
        .get(HOST)
        .ok_or_else(|| TerminalError::ForbiddenOrigin("Host header is required".to_owned()))?
        .to_str()
        .map_err(|_| TerminalError::ForbiddenOrigin("Host is not valid ASCII".to_owned()))?
        .parse::<http::uri::Authority>()
        .map_err(|_| TerminalError::ForbiddenOrigin("Host is invalid".to_owned()))?;
    if !origin_authority.host().eq_ignore_ascii_case(host.host())
        || origin_authority.port_u16() != host.port_u16()
    {
        return Err(TerminalError::ForbiddenOrigin(format!(
            "Origin authority {origin_authority} does not match Host {host}"
        )));
    }
    Ok(())
}
