use super::super::tests::notification;
use super::*;

fn card(message: &Value) -> Value {
    serde_json::from_str(message["content"].as_str().unwrap()).unwrap()
}

#[test]
fn metadata_precedes_the_complete_reply_and_is_plain_text() {
    let mut event = notification("## 结果\n\n完成检查。\n\n```rust\nfn main() {}\n```\n");
    event.title = "<at id=all></at> *literal title*".into();
    let messages = messages(&event, "ou_owner", "delivery").unwrap();
    assert_eq!(messages.len(), 1);
    let card = card(&messages[0]);
    assert_eq!(
        card["header"]["title"]["content"],
        "AOW·<at id=all></at> *literal title*·Codex·完成"
    );
    let elements = card["body"]["elements"].as_array().unwrap();
    let metadata = elements[0]["columns"][0]["elements"].as_array().unwrap();
    let text = metadata
        .iter()
        .map(|element| {
            assert_eq!(element["text"]["tag"], "plain_text");
            element["text"]["content"].as_str().unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    for value in ["Tab：检查", &event.title, "Session ID：session-1"] {
        assert!(text.contains(value), "missing {value}");
    }
    for value in ["Agent：", "项目：", "工作目录：", &event.cwd] {
        assert!(!text.contains(value), "unexpected {value}");
    }
    assert_eq!(elements[1]["content"], "**本轮结论**");
    assert_eq!(elements[2]["content"].as_str(), event.conclusion.as_deref());
}

#[test]
fn tab_links_are_independent_escaped_and_repeated_on_every_card() {
    let mut event = notification(&"结论\n\n".repeat(7000));
    event.sources[0].tab_name = "检查 [x] <at id=all>".into();
    event.sources[0].tab_url = Some("https://aow.example.com/aow/tabs/first".into());
    let mut second = event.sources[0].clone();
    second.tab_name = "另一个 Tab".into();
    second.tab_url = Some("https://aow.example.com/aow/tabs/second".into());
    event.sources.push(second);
    let messages = messages(&event, "ou_owner", "delivery").unwrap();
    assert!(messages.len() > 1);
    for message in messages {
        assert!(size(&message).unwrap() <= MAX_REQUEST_BYTES);
        let card = card(&message);
        let elements = card["body"]["elements"][0]["columns"][0]["elements"]
            .as_array()
            .unwrap();
        assert_eq!(elements[0]["text"]["tag"], "lark_md");
        assert_eq!(
            elements[0]["text"]["content"],
            "Tab：[检查 \\[x\\] &lt;at id=all&gt;](https://aow.example.com/aow/tabs/first)"
        );
        assert_eq!(
            elements[1]["text"]["content"],
            "Tab：[另一个 Tab](https://aow.example.com/aow/tabs/second)"
        );
        assert_eq!(elements[2]["text"]["tag"], "plain_text");
    }
}

#[test]
fn long_unicode_and_json_escapes_split_within_the_actual_request_budget() {
    // The short-looking quotes/backslashes cost extra bytes in both JSON layers.
    let text = "一段结果 🦀 \\\"quoted\\\"\n\n".repeat(8000);
    let event = notification(&text);
    let messages = messages(&event, "ou_owner", "00000000-0000-4000-8000-000000000001").unwrap();
    assert!(messages.len() > 2);
    let mut reconstructed = String::new();
    let mut ids = std::collections::HashSet::new();
    let mut previous_metadata = None;
    for (index, message) in messages.iter().enumerate() {
        assert!(size(message).unwrap() <= MAX_REQUEST_BYTES);
        assert_eq!(message["msg_type"], "interactive");
        let id = message["uuid"].as_str().unwrap();
        assert!(id.len() <= 50);
        assert!(ids.insert(id));
        let card = card(message);
        assert_eq!(
            card["header"]["title"]["content"],
            format!("AOW·完成检查·Codex·完成 · {}/{}", index + 1, messages.len())
        );
        let elements = &card["body"]["elements"];
        if let Some(previous) = &previous_metadata {
            assert_eq!(previous, &elements[0]);
        }
        previous_metadata = Some(elements[0].clone());
        let fragment = elements[2]["content"].as_str().unwrap();
        assert!(fragment.ends_with("\n\n"));
        reconstructed.push_str(fragment);
    }
    assert_eq!(reconstructed, text);
}

#[test]
fn giant_lines_and_code_blocks_preserve_all_content_and_balanced_fences() {
    for delimiter in ["```", "~~~~"] {
        for code in ["let 中文 = \"🦀\";\n".repeat(5000), "中文🦀".repeat(10000)] {
            let text = format!("前言\n\n{delimiter}rust\n{code}\n{delimiter}\n\n结束");
            let messages = messages(&notification(&text), "ou_owner", "delivery").unwrap();
            assert!(messages.len() > 2);
            let mut reconstructed = String::new();
            for message in messages {
                assert!(size(&message).unwrap() <= MAX_REQUEST_BYTES);
                let card = card(&message);
                let fragment = card["body"]["elements"][2]["content"].as_str().unwrap();
                assert_eq!(
                    fragment
                        .lines()
                        .filter(|line| line.starts_with(delimiter))
                        .count()
                        % 2,
                    0
                );
                reconstructed.extend(fragment.lines().filter(|line| !line.starts_with(delimiter)));
            }
            let expected: String = text
                .lines()
                .filter(|line| !line.starts_with(delimiter))
                .collect();
            assert_eq!(reconstructed, expected);
        }
    }
}

#[test]
fn no_conclusion_is_explicit_and_oversized_metadata_fails_before_delivery() {
    let mut event = notification("");
    event.conclusion = None;
    let message = messages(&event, "ou_owner", "delivery").unwrap();
    assert_eq!(
        card(&message[0])["body"]["elements"][2]["content"],
        "未捕获到本轮结论。"
    );
    event.sources[0].project_name = "项目".repeat(10_000);
    assert!(
        messages(&event, "ou_owner", "delivery")
            .unwrap_err()
            .to_string()
            .contains("基础信息过长")
    );
}
