use super::*;

pub(super) fn validate_display_name(value: &str) -> Result<String, AowError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 120 || value.chars().any(char::is_control) {
        return Err(AowError::Invalid(
            "display name must contain 1-120 printable characters".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

pub(super) fn validate_id(value: &str) -> Result<String, AowError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 80
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(AowError::Invalid(
            "agent id may only contain letters, numbers, '-' and '_'".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

pub(super) fn validate_command(value: &str) -> Result<String, AowError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(AowError::Invalid("agent command is invalid".to_owned()));
    }
    if value.split_whitespace().count() != 1 {
        return Err(AowError::Invalid(
            "agent command must be one executable; put flags in args".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

pub(super) fn validate_arguments(values: &[String]) -> Result<(), AowError> {
    if values.len() > 128
        || values
            .iter()
            .any(|value| value.len() > 8192 || value.contains('\0'))
    {
        return Err(AowError::Invalid("agent arguments are invalid".to_owned()));
    }
    Ok(())
}

pub(super) fn validate_env_keys(values: &[String]) -> Result<(), AowError> {
    if values.len() > 128
        || values.iter().any(|value| {
            value.is_empty()
                || value.len() > 256
                || !value
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
    {
        return Err(AowError::Invalid(
            "agent environment keys are invalid".to_owned(),
        ));
    }
    Ok(())
}
