use super::*;

pub(super) fn validate_display_name(value: &str) -> Result<String, AowError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 120 || value.chars().any(char::is_control) {
        return Err(AowError::Invalid(
            aow_i18n::text("validation.display_name_must_contain_1_120_printable_characters")
                .to_owned(),
        ));
    }
    Ok(value.to_owned())
}

pub(super) fn validate_id(value: &str) -> Result<String, AowError> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(AowError::Invalid(
            aow_i18n::text("server.agent_id_is_required_1_80_letters_digits_hyphens").to_owned(),
        ));
    }
    Ok(value.to_owned())
}

pub(super) fn validate_command(value: &str) -> Result<String, AowError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(AowError::Invalid(
            aow_i18n::text("validation.agent_command_is_invalid").to_owned(),
        ));
    }
    if value.split_whitespace().count() != 1 {
        return Err(AowError::Invalid(
            aow_i18n::text("validation.agent_command_must_be_one_executable_put_flags_in_args")
                .to_owned(),
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
        return Err(AowError::Invalid(
            aow_i18n::text("validation.agent_arguments_are_invalid").to_owned(),
        ));
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
            aow_i18n::text("validation.agent_environment_keys_are_invalid").to_owned(),
        ));
    }
    Ok(())
}
