use super::*;

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

#[test]
fn pi_accepts_its_empty_editor_but_not_working_dialog_or_unconfigured_screens() {
    for screen in [
        "Welcome to Pi\n────────\n\n────────\n/workspace\n0.0%/200k (auto)   model",
        "────────\n  \n────────\n~/repo (main) • Session\n?/200k (auto)    model • high\nstatus",
    ] {
        assert!(InteractiveAgent::Pi.input_ready(&lines(screen)), "{screen}");
    }
    for screen in [
        "── Working ──\n\n────────\n/workspace\n0.0%/200k model",
        "────────\nTrust this project?\n────────\n/workspace\n0.0%/200k model",
        "────────\n\n────────\n/workspace\n0.0%/0 no-model",
        "────────\nExisting input\n────────\n/workspace\n0.0%/200k model",
    ] {
        assert!(
            !InteractiveAgent::Pi.input_ready(&lines(screen)),
            "{screen}"
        );
    }
}

#[test]
fn hermes_accepts_idle_cli_prompts_but_not_approval_clarification_or_working_states() {
    let agent = InteractiveAgent::Hermes;
    for screen in [
        "Welcome to Hermes\n────────\n❯\n────────",
        "Ready\n│ work-profile ❯   │\n",
    ] {
        assert!(agent.input_ready(&lines(screen)), "{screen}");
    }
    for screen in [
        "Loading Hermes",
        "⚠ ❯",
        "? ❯",
        "🔐 ❯",
        "✎ ❯",
        "⚕ ❯ msg=interrupt · /queue",
        "❯ type your answer here and press Enter",
    ] {
        assert!(!agent.input_ready(&lines(screen)), "{screen}");
    }
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
fn traecli_footer_probe_checks_loading_only_in_the_model_segment() {
    let agent = InteractiveAgent::TraeCli;
    for footer in [
        "gpt-6-astra xhigh · /workspace/loading-fix · for agents",
        "GPT-5.6-Sol xhigh · Context 100% left · /workspace · loading-fix",
    ] {
        assert!(agent.input_ready(&lines(footer)), "{footer}");
    }
    for footer in [
        "loading · /workspace · for agents",
        "model: Loading… · Context 100% left · /workspace",
        " · /workspace · for agents",
    ] {
        assert!(!agent.input_ready(&lines(footer)), "{footer}");
    }
}

#[test]
fn codex_accepts_a_directory_in_either_of_the_last_two_content_lines() {
    let agent = InteractiveAgent::Codex;
    for footer in [
        "GPT-6-Astra xhigh · /workspace",
        "GPT-6-Astra xhigh · ~/workspace",
        "GPT-6-Astra xhigh · /workspace/loading-fix · for agents",
        "GPT-6-Astra xhigh · Context 100% left · /workspace",
        " · /workspace",
    ] {
        for suffix in ["", "\n任意状态提示", "\n\n任意状态提示\n\n"] {
            let screen = format!("› Ask Codex to do anything\n{footer}{suffix}");
            assert!(agent.input_ready(&lines(&screen)), "{screen}");
        }
    }
    for screen in [
        "",
        "model: loading\n› Ask Codex to do anything",
        "GPT-6-Astra xhigh · workspace",
        "GPT-6-Astra xhigh · ~",
        "GPT-6-Astra xhigh · Context 100% left",
        "GPT-6-Astra xhigh /workspace",
        "GPT-6-Astra xhigh · directory: /workspace",
        "GPT-6-Astra xhigh · /workspace · for agents\nlogin\nrequired",
        "GPT-6-Astra xhigh · ~/workspace\n\nlogin\n\nrequired\n\n",
    ] {
        assert!(!agent.input_ready(&lines(screen)), "{screen}");
    }
}

#[test]
fn codex_directory_footer_is_independent_of_shortcuts_and_warnings() {
    let agent = InteractiveAgent::Codex;
    for shortcuts in [
        "  ? for shortcuts",
        "  ? for shortcuts · 100% context left",
        "  ? for shortcuts  ⚠ 1 warning · f2 to view",
        "  ? for shortcuts                                      ⚠ 2 warnings · f2 to view",
    ] {
        let screen = format!(
            "› Ask Codex to do anything\n\n  GPT-6-Astra xhigh · ~/workspace/.aow-inbox-task\n{shortcuts}\n\n"
        );
        assert!(agent.input_ready(&lines(&screen)), "{screen}");
    }
    for screen in [
        "GPT-6-Astra xhigh · Context 100% left\n  ? for shortcuts  ⚠ 1 warning · f2 to view",
        "? for shortcuts  ⚠ 1 warning · f2 to view",
        "GPT-6-Astra xhigh · ~/workspace\n  ? for shortcuts  ⚠ 1 warning · f2 to view\nlogin\nrequired\ncontinue",
    ] {
        assert!(!agent.input_ready(&lines(screen)), "{screen}");
    }
}
