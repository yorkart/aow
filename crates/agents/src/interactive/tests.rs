use super::*;

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

#[test]
fn footer_probe_ignores_loading_and_scrollback_noise() {
    let agent = InteractiveAgent::TraeCli;
    assert!(!agent.input_ready(&lines(
        "model: loading\n❯ Ask anything\n? for shortcuts\n100% context left"
    )));
    assert!(agent.input_ready(&lines(
        "❯ Ask anything\nGPT-5.6-Sol xhigh · Context 100% left · /workspace\n\n"
    )));
    assert!(InteractiveAgent::Codex.input_ready(&lines(
        "❯ Ask anything\ngpt-6-astra xhigh · /workspace · for agents"
    )));
    assert!(!agent.input_ready(&lines(
        "earlier · output · noise\nloading\ninput\nfooter\nlast"
    )));
    assert!(!agent.input_ready(&lines("one ·\ntwo ·")));
}

#[test]
fn footer_probe_checks_loading_only_in_the_model_segment() {
    for agent in [InteractiveAgent::Codex, InteractiveAgent::TraeCli] {
        for footer in [
            "gpt-6-astra xhigh · /workspace/loading-fix · for agents",
            "GPT-5.6-Sol xhigh · Context 100% left · /workspace · loading-fix",
        ] {
            assert!(agent.input_ready(&lines(footer)), "{agent:?}: {footer}");
        }
        for footer in [
            "loading · /workspace · for agents",
            "model: Loading… · Context 100% left · /workspace",
            " · /workspace · for agents",
        ] {
            assert!(!agent.input_ready(&lines(footer)), "{agent:?}: {footer}");
        }
    }
}
