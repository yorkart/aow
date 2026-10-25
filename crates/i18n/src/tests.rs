use crate::*;

#[tokio::test]
async fn defaults_to_english_and_keeps_requests_isolated() {
    assert_eq!(current_language(), Language::English);
    assert_eq!(text("server.untitled_project"), "Untitled project");
    let (english, chinese) = tokio::join!(
        scope(Language::English, async {
            tokio::task::yield_now().await;
            text("server.untitled_project")
        }),
        scope(Language::Chinese, async {
            tokio::task::yield_now().await;
            text("server.untitled_project")
        })
    );
    assert_eq!((english, chinese), ("Untitled project", "未命名项目"));
    assert_eq!(current_language(), Language::English);
}

#[tokio::test]
async fn carries_language_to_async_and_blocking_work() {
    scope(Language::Chinese, async {
        assert_eq!(
            spawn(async { text("server.untitled_project") })
                .await
                .unwrap(),
            "未命名项目"
        );
        assert_eq!(
            spawn_blocking(|| text("server.untitled_project"))
                .await
                .unwrap(),
            "未命名项目"
        );
    })
    .await;
    assert_eq!(
        spawn_blocking(current_language).await.unwrap(),
        Language::English
    );
}

#[test]
fn interpolation_does_not_reinterpret_user_text() {
    let result = message(
        "server.completed",
        &[
            ("p0", "{{auto0}}".into()),
            ("auto0", "<b>task</b>".into()),
            ("p2", "Agent".into()),
        ],
    );
    assert_eq!(result, "{{auto0}}·<b>task</b>·Agent·Completed");
    assert_eq!(Language::parse("fr-FR"), Language::English);
    assert_eq!(Language::parse("zh_Hans"), Language::Chinese);
    assert_eq!(text("unknown.key"), "The operation could not be completed.");
}

#[test]
fn unsupported_persisted_language_falls_back_to_english() {
    assert_eq!(
        serde_json::from_str::<Language>("\"fr-FR\"").unwrap(),
        Language::English
    );
    assert_eq!(
        serde_json::from_str::<Language>("\"zh_Hans\"").unwrap(),
        Language::Chinese
    );
    assert_eq!(
        serde_json::to_string(&Language::Chinese).unwrap(),
        "\"zh-CN\""
    );
}
