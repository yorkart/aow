//! Business events become transport-independent messages here.
use aow_im::{Field, Message};

use super::AutomationFailureNotification;
use crate::terminal::notifications::TaskStopNotification;

pub(super) fn task_completed(event: &TaskStopNotification) -> Message {
    let agent = aow_agents::Agent::from_id(&event.agent)
        .map(|agent| agent.definition().display_name)
        .unwrap_or(&event.agent);
    let mut projects = Vec::new();
    let mut fields = Vec::new();
    for source in &event.sources {
        let project = source.project_name.trim();
        if !project.is_empty() && !projects.contains(&project) {
            projects.push(project);
        }
        fields.push(Field {
            label: "Tab".into(),
            value: source.tab_name.clone(),
            url: source.tab_url.clone(),
        });
    }
    let project = if projects.is_empty() {
        aow_i18n::text("server.untitled_project").into()
    } else {
        projects.join("、")
    };
    fields.push(Field {
        label: aow_i18n::text("server.session").into(),
        value: format!("{}\nSession ID: {}", event.title, event.session_id),
        url: None,
    });
    if let Some(usage) = event.usage {
        fields.push(Field {
            label: aow_i18n::text("server.tokens_this_turn").into(),
            value: aow_i18n::message(
                "server.total_input_output_cache_hits_included_in_input",
                &[
                    ("auto0", format_token_count(usage.total_tokens).to_string()),
                    ("auto1", format_token_count(usage.input_tokens).to_string()),
                    ("auto2", format_token_count(usage.output_tokens).to_string()),
                    (
                        "auto3",
                        format_token_count(usage.cached_input_tokens).to_string(),
                    ),
                ],
            ),
            url: None,
        });
    }
    Message {
        title: aow_i18n::message(
            "server.completed",
            &[
                ("p0", project.to_string()),
                ("auto0", event.title.to_string()),
                ("p2", agent.to_string()),
            ],
        ),
        fields,
        body_label: aow_i18n::text("server.final_response").into(),
        body: event
            .conclusion
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .unwrap_or(aow_i18n::text(
                "server.no_final_response_captured_for_this_turn",
            ))
            .into(),
        markdown: true,
        error: false,
    }
}

fn format_token_count(count: u64) -> String {
    if count < 1_000 {
        return format!("{count} tokens");
    }
    let units = ["K", "M", "B", "T"];
    let count = u128::from(count);
    let mut scale = 1_000;
    let mut unit = 0;
    let mut tenths = (count * 10 + scale / 2) / scale;
    while tenths >= 10_000 && unit + 1 < units.len() {
        scale *= 1_000;
        unit += 1;
        tenths = (count * 10 + scale / 2) / scale;
    }
    let value = if tenths % 10 == 0 {
        (tenths / 10).to_string()
    } else {
        format!("{}.{}", tenths / 10, tenths % 10)
    };
    format!("{value}{} tokens", units[unit])
}

pub(super) fn automation_failure(event: &AutomationFailureNotification) -> Message {
    let run = &event.run;
    let mut fields = vec![Field {
        label: aow_i18n::text("server.task").into(),
        value: aow_i18n::message(
            "server.agent_task_id_run_id_finished_duration_exit_code",
            &[
                ("auto0", run.task_name.to_string()),
                ("auto1", run.agent.id().to_string()),
                ("auto2", run.task_id.to_string()),
                ("auto3", run.id.to_string()),
                (
                    "auto4",
                    run.finished_at
                        .map(|at| at.to_rfc3339())
                        .unwrap_or_else(|| aow_i18n::text("server.unknown").into())
                        .to_string(),
                ),
                (
                    "auto5",
                    run.duration_ms
                        .map(|ms| {
                            aow_i18n::message(
                                "server.seconds",
                                &[("auto0", format!("{:.1}", ms as f64 / 1000.0))],
                            )
                        })
                        .unwrap_or_else(|| aow_i18n::text("server.unknown").into())
                        .to_string(),
                ),
                (
                    "auto6",
                    run.exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| aow_i18n::text("server.none_2").into())
                        .to_string(),
                ),
            ],
        ),
        url: None,
    }];
    if let Some(url) = &event.run_url {
        fields.push(Field {
            label: aow_i18n::text("server.run_record").into(),
            value: aow_i18n::text("server.view_run_record").into(),
            url: Some(url.clone()),
        });
    }
    Message {
        title: aow_i18n::message(
            "server.automation_failed",
            &[("auto0", run.task_name.to_string())],
        ),
        fields,
        body_label: aow_i18n::text("server.failure_reason").into(),
        body: run
            .message
            .as_deref()
            .unwrap_or(aow_i18n::text("server.no_failure_reason_recorded"))
            .chars()
            .take(2000)
            .collect(),
        markdown: false,
        error: true,
    }
}
