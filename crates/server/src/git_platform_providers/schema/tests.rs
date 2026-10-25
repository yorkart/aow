use super::*;
use crate::{AppState, BasePath, auth};
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn public_schema_is_anonymous_static_and_does_not_open_other_provider_routes() {
    let directory = tempfile::tempdir().unwrap();
    auth::write_credentials(directory.path(), "admin", "test-password");
    for prefix in ["", "/tools/aow"] {
        let mut state = AppState::new(directory.path().to_owned())
            .with_base_path(BasePath::parse(prefix).unwrap());
        state.auth = auth::AuthService::persistent(directory.path());
        let app = crate::build_router(state);
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("{prefix}{PUBLIC_PATH}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "application/json; charset=utf-8"
        );
        assert!(!response.headers().contains_key("set-cookie"));
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert_eq!(body.as_ref(), DOCUMENT.as_bytes());
        let response = app
            .clone()
            .oneshot(
                Request::head(format!("{prefix}{PUBLIC_PATH}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty()
        );
        for (method, path) in [
            (Method::POST, PUBLIC_PATH),
            (Method::PUT, PUBLIC_PATH),
            (
                Method::GET,
                "/api/public/git-platform-provider-schema/private",
            ),
            (Method::GET, "/api/aow/git-platform-providers"),
            (Method::POST, "/api/aow/git-platform-providers/test"),
            (Method::GET, "/api/my-pull-requests?repo=/private"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(format!("{prefix}{path}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
    }
}

#[test]
fn public_contract_has_resolvable_definitions_for_every_read_operation() {
    let document: Value = serde_json::from_str(DOCUMENT).unwrap();
    assert_eq!(document["protocol_version"], VERSION);
    fn references(value: &Value, root: &Value) {
        match value {
            Value::Object(map) => {
                if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
                    assert!(
                        root.pointer(reference.strip_prefix('#').unwrap()).is_some(),
                        "unresolved {reference}"
                    );
                }
                for value in map.values() {
                    references(value, root);
                }
            }
            Value::Array(values) => {
                for value in values {
                    references(value, root);
                }
            }
            _ => {}
        }
    }
    references(&document, &document);
    let names: Vec<_> = document["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    for tool in document["tools"].as_array().unwrap() {
        assert_eq!(tool["required"], tool["name"] == "describe");
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
    }
    assert_eq!(
        names,
        vec![
            "describe",
            "pull_requests.list",
            "pull_requests.detail",
            "pull_requests.diff",
            "repository.info",
            "repository.commit_links"
        ]
    );
    let defaults = crate::git_platform_providers::bundled::load().unwrap();
    assert_eq!(
        defaults.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        vec!["github.mjs", "gitlab.mjs"]
    );
    for provider in defaults {
        assert!(!provider.name.is_empty());
        assert!(!provider.hosts.is_empty());
    }
}

#[tokio::test]
async fn old_provider_routes_are_not_aliases() {
    let app = crate::build_router(AppState::new(std::path::PathBuf::new()));
    for path in [
        "/api/public/review-provider-schema",
        "/api/aow/review-providers",
        "/api/review-targets",
    ] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}
