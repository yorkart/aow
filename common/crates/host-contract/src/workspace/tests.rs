use super::*;

#[test]
fn file_kind_uses_snake_case() {
    assert_eq!(
        serde_json::to_string(&FileKind::Directory).unwrap(),
        "\"directory\""
    );
}
