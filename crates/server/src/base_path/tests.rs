use super::*;
use crate::{AppState, auth::PinAuth, build_router};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;

fn fixture(base: &str) -> (tempfile::TempDir, AppState) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("assets")).unwrap();
    std::fs::write(root.path().join("index.html"), "<!doctype html><html><head><script type=\"module\" src=\"./assets/app.js\"></script></head><body></body></html>").unwrap();
    std::fs::write(
        root.path().join("assets/app.js"),
        "export const ready = true;",
    )
    .unwrap();
    std::fs::write(
        root.path().join("pin.md5"),
        format!("{:x}", md5::compute("123456")),
    )
    .unwrap();
    let mut state = AppState::new(root.path().to_path_buf());
    state.auth = PinAuth::persistent(root.path());
    (root, state.with_base_path(BasePath::parse(base).unwrap()))
}

async fn get(app: &Router, path: &str) -> Response {
    app.clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

#[test]
fn validates_and_normalizes_deployment_paths() {
    for value in ["", "/"] {
        assert_eq!(BasePath::parse(value).unwrap(), BasePath::default());
    }
    assert_eq!(
        BasePath::parse("/tools/aow/").unwrap().as_str(),
        "/tools/aow"
    );
    for invalid in [
        "tools/aow",
        "//host",
        "/a//b",
        "/a/../b",
        "/./b",
        "/a%2fb",
        "/a?b",
        "/a#b",
        "/a b",
        "/a\\b",
        "/a\"b",
        "/中文",
        "/a//",
    ] {
        assert!(BasePath::parse(invalid).is_err(), "accepted {invalid}");
    }
}

#[tokio::test]
async fn one_frontend_build_serves_root_and_subpath_including_deep_links() {
    for base in ["", "/tools/aow"] {
        let (_root, state) = fixture(base);
        let app = build_router(state);
        for path in [
            "/aow/",
            "/aow/tabs/terminal/tab-1",
            "/m/",
            "/share/token",
            "/index.html",
        ] {
            let response = get(&app, &format!("{base}{path}")).await;
            assert_eq!(response.status(), StatusCode::OK, "{base}{path}");
            let html = body(response).await;
            assert!(html.contains(&format!("<base href=\"{base}/\">")));
            assert!(html.contains(&format!("name=\"aow-base-path\" content=\"{base}\"")));
            assert!(html.contains("./assets/app.js"));
        }
        let response = get(&app, &format!("{base}/assets/app.js")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body(response).await, "export const ready = true;");
        let redirect = get(&app, &format!("{base}/?ui=mobile")).await;
        assert_eq!(
            redirect.headers()[LOCATION],
            format!("{base}/aow/?ui=mobile")
        );
        assert_eq!(
            get(&app, &format!("{base}/api/health")).await.status(),
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn mount_is_bounded_and_preserves_authentication_and_cookie_scope() {
    let (_root, state) = fixture("/tools/aow");
    let root_app = build_router(state.clone().with_base_path(BasePath::default()));
    let app = build_router(state);
    let redirect = get(&app, "/tools/aow?ui=desktop").await;
    assert_eq!(redirect.headers()[LOCATION], "/tools/aow/?ui=desktop");
    for path in [
        "/api/health",
        "/aow/",
        "/assets/app.js",
        "/tools/aow-other/api/health",
    ] {
        assert_eq!(
            get(&app, path).await.status(),
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    for path in [
        "/api/fs/tree",
        "/api/operations/stream",
        "/api/terminals/task-stops",
        "/help",
        "/fs/",
    ] {
        assert_eq!(
            get(&app, &format!("/tools/aow{path}")).await.status(),
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
    }
    // The exact public read route still bypasses login; other API routes do not.
    assert_eq!(
        get(&app, "/tools/aow/api/public/session-shares/missing")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app, "/tools/aow/api/public/session-shares/missing/extra")
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let login = app
        .clone()
        .oneshot(
            Request::post("/tools/aow/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"pin":"123456"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.starts_with("aow_session_"));
    assert!(cookie.contains("Path=/tools/aow/; HttpOnly; SameSite=Strict"));
    let cookie = cookie.split(';').next().unwrap();
    for (router, path, status) in [
        (&app, "/tools/aow/api/fs/tree", StatusCode::OK),
        (&root_app, "/api/fs/tree", StatusCode::UNAUTHORIZED),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    for (path, expected) in [
        ("/help", "/tools/aow/api/fs/tree"),
        ("/fs/", "/tools/aow/fs/tmp/"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/tools/aow{path}"))
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body(response).await.contains(expected));
    }
}
