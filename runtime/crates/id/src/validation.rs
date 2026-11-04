/// Validate an opaque application ID without depending on its generator or encoding.
pub fn is_valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_opaque_ids_and_rejects_unsafe_components() {
        for id in [
            "A",
            "0",
            "0001",
            "Config_v2-A",
            "550e8400-e29b-41d4-a716-446655440000",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        ] {
            assert!(is_valid_id(id), "{id}");
        }
        assert!(is_valid_id(&"a".repeat(128)));
        assert!(!is_valid_id(&"a".repeat(129)));
        for id in [
            "",
            ".",
            "..",
            "../escape",
            "a/b",
            "a\\b",
            "a.b",
            "a b",
            "a\n",
            "a\0",
            "%2f",
            "中文",
        ] {
            assert!(!is_valid_id(id), "{id:?}");
        }
    }
}
