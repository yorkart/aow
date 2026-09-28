use crate::paths::{attach_uri, encode_id, runtime_path};

#[test]
fn runtime_ids_are_encoded_as_one_path_segment() {
    assert_eq!(runtime_path("a/b c"), "/v1/runtimes/a%2Fb%20c");
    assert_eq!(
        encode_id("aZ09-._~:/?#[]@!$&'()*+,;=%é"),
        "aZ09-._~%3A%2F%3F%23%5B%5D%40%21%24%26%27%28%29%2A%2B%2C%3B%3D%25%C3%A9"
    );
}

#[test]
fn attach_uri_encodes_the_id_and_optional_offset() {
    assert_eq!(
        attach_uri("a/b c", Some("epoch/one"), Some(42), false, false, false),
        "ws://aow-terminald/v1/runtimes/a%2Fb%20c/attach?epoch=epoch%2Fone&after=42"
    );
    assert_eq!(
        attach_uri("runtime", None, None, false, false, false),
        "ws://aow-terminald/v1/runtimes/runtime/attach"
    );
    assert_eq!(
        attach_uri("runtime", None, Some(42), false, false, false),
        "ws://aow-terminald/v1/runtimes/runtime/attach"
    );
    assert_eq!(
        attach_uri("runtime", None, None, true, false, false),
        "ws://aow-terminald/v1/runtimes/runtime/attach?control=v2"
    );
    assert_eq!(
        attach_uri("runtime", Some("epoch"), Some(42), true, false, false),
        "ws://aow-terminald/v1/runtimes/runtime/attach?epoch=epoch&after=42&control=v2"
    );
    assert_eq!(
        attach_uri("runtime", None, None, true, true, false),
        "ws://aow-terminald/v1/runtimes/runtime/attach?control=v2&capabilities=vt-snapshot-v1"
    );
    assert_eq!(
        attach_uri("runtime", None, None, true, true, true),
        "ws://aow-terminald/v1/runtimes/runtime/attach?control=v2&capabilities=vt-snapshot-v1&observer=v1"
    );
}
