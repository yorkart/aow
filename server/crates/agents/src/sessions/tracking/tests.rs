use super::*;
use serde_json::json;

#[test]
fn codex_like_freezes_each_turn_and_prefers_the_completion_payload() {
    for agent in [Agent::Codex, Agent::TraeCli] {
        let mut parser = agent.session_tracking().unwrap().task_stop_parser();
        let mut consume = |record| parser.consume("session", &record);
        assert!(
            consume(json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"one"}}))
                .is_none()
        );
        consume(
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"旧版本"}]}}),
        );
        let first = consume(json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"one","last_agent_message":"最终结论\n```rust\nfn main() {}\n```"}})).unwrap();
        consume(
            json!({"type":"event_msg","payload":{"type":"agent_message","phase":"commentary","message":"进行中"}}),
        );
        let second =
            consume(json!({"type":"event_msg","payload":{"type":"turn_complete","turn_id":"two"}}))
                .unwrap();
        assert_eq!(
            first.conclusion.as_deref(),
            Some("最终结论\n```rust\nfn main() {}\n```")
        );
        assert_eq!(second.conclusion, None);
        assert_eq!(first.turn_id.as_deref(), Some("one"));
    }
}

#[test]
fn codex_like_fallbacks_do_not_cross_turns_or_reuse_aborted_responses() {
    let mut parser = TrackingAgent::Codex.task_stop_parser();
    for reply in [
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"结果"}]}}),
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"结果"}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","phase":"final_answer","content":[{"type":"text","text":"结果"}]}}}),
    ] {
        for boundary in [
            None,
            Some("task_started"),
            Some("turn_aborted"),
            Some("user_message"),
            Some("error"),
        ] {
            parser.consume("s", &reply);
            if let Some(kind) = boundary {
                parser.consume("s", &json!({"type":"event_msg","payload":{"type":kind}}));
            }
            let event = parser
                .consume(
                    "s",
                    &json!({"type":"event_msg","payload":{"type":"task_complete"}}),
                )
                .unwrap();
            assert_eq!(
                event.conclusion.as_deref(),
                if boundary.is_none() {
                    Some("结果")
                } else {
                    None
                }
            );
        }
    }
    parser.consume(
        "s",
        &json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"old"}}),
    );
    parser.consume(
        "s",
        &json!({"type":"event_msg","payload":{"type":"agent_message","message":"旧结论"}}),
    );
    assert_eq!(
        parser
            .consume(
                "s",
                &json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"new"}})
            )
            .unwrap()
            .conclusion,
        None
    );
}

#[test]
fn claude_captures_only_the_final_reply_after_hook_continuations() {
    let mut parser = TrackingAgent::Claude.task_stop_parser();
    let assistant = |text, reason| json!({"type":"assistant","sessionId":"s","message":{"stop_reason":reason,"content":[{"type":"thinking","thinking":"private"},{"type":"text","text":text}]}});
    let boundary =
        json!({"type":"system","subtype":"turn_duration","sessionId":"s","uuid":"boundary"});
    assert!(
        parser
            .consume("s", &assistant("第一次答复", "end_turn"))
            .is_none()
    );
    parser.consume(
        "s",
        &json!({"type":"user","isMeta":true,"message":{"content":"Stop hook continuation"}}),
    );
    parser.consume("s", &assistant("工具执行中", "tool_use"));
    assert!(
        parser
            .consume("s", &assistant("本轮最终结论", "end_turn"))
            .is_none()
    );
    let mut foreign = assistant("不属于本会话", "end_turn");
    foreign["sessionId"] = json!("another");
    parser.consume("s", &foreign);
    foreign["sessionId"] = json!("s");
    foreign["isSidechain"] = json!(true);
    parser.consume("s", &foreign);
    let event = parser.consume("s", &boundary).unwrap();
    assert_eq!(event.conclusion.as_deref(), Some("本轮最终结论"));
    assert_eq!(parser.consume("s", &boundary).unwrap().conclusion, None);
    parser.consume("s", &assistant("已被下一轮取代", "end_turn"));
    parser.consume(
        "s",
        &json!({"type":"user","message":{"content":"下一个问题"}}),
    );
    assert_eq!(parser.consume("s", &boundary).unwrap().conclusion, None);
}

#[tokio::test]
async fn title_tracking_survives_missing_pid_while_exact_tracking_requires_it() {
    let environment = SessionEnvironment::new();
    let context = || LiveSessionContext {
        pid: None,
        cwd: "/workspace/demo",
        title: "⠋ A running task | demo",
        environment: &environment,
    };
    for agent in [Agent::Codex, Agent::TraeCli] {
        let tracker = agent.session_tracking().unwrap();
        assert_eq!(tracker.agent(), agent);
        assert_eq!(
            tracker.resolve_live_session(context()).await,
            SessionResolution::Resolved(SessionTarget::Title("A running task".into()))
        );
        assert_eq!(
            tracker
                .resolve_live_session(LiveSessionContext {
                    title: "Working | demo",
                    ..context()
                })
                .await,
            SessionResolution::NotFound
        );
    }
    assert_eq!(
        Agent::Claude
            .session_tracking()
            .unwrap()
            .resolve_live_session(context())
            .await,
        SessionResolution::NotFound
    );
    assert_eq!(Agent::Gemini.session_tracking(), None);
}

#[test]
fn claude_combines_text_blocks_of_one_message_but_excludes_tool_prologues() {
    let mut parser = TrackingAgent::Claude.task_stop_parser();
    for record in [
        json!({"type":"assistant","message":{"id":"a","content":[{"type":"text","text":"执行工具之前"}]}}),
        json!({"type":"assistant","message":{"id":"a","content":[{"type":"tool_use","name":"bash"}]}}),
        json!({"type":"assistant","message":{"id":"b","content":[{"type":"thinking","thinking":"private"}]}}),
        json!({"type":"assistant","message":{"id":"b","content":[{"type":"text","text":"第一部分"}]}}),
        json!({"type":"assistant","message":{"id":"b","content":[{"type":"text","text":"第二部分"}]}}),
        json!({"type":"assistant","message":{"id":"b","stop_reason":"end_turn","content":[]}}),
    ] {
        assert!(parser.consume("s", &record).is_none());
    }
    let event = parser
        .consume("s", &json!({"type":"system","subtype":"turn_duration"}))
        .unwrap();
    assert_eq!(event.conclusion.as_deref(), Some("第一部分\n\n第二部分"));
}
