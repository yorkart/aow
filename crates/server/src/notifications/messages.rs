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
        "未命名项目".into()
    } else {
        projects.join("、")
    };
    fields.push(Field {
        label: "会话".into(),
        value: format!("{}\nSession ID：{}", event.title, event.session_id),
        url: None,
    });
    if let Some(usage) = event.usage {
        fields.push(Field {
            label: "本轮 Token".into(),
            value: format!(
                "合计：{}\n输入：{} · 输出：{}\n缓存命中：{}（包含在输入中）",
                format_token_count(usage.total_tokens),
                format_token_count(usage.input_tokens),
                format_token_count(usage.output_tokens),
                format_token_count(usage.cached_input_tokens),
            ),
            url: None,
        });
    }
    Message {
        title: format!("{project}·{}·{agent}·完成", event.title),
        fields,
        body_label: "本轮结论".into(),
        body: event
            .conclusion
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .unwrap_or("未捕获到本轮结论。")
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
        label: "任务".into(),
        value: format!(
            "{}\nAgent：{}\n任务 ID：{}\n执行 ID：{}\n结束时间：{}\n耗时：{}\n退出码：{}",
            run.task_name,
            run.agent.id(),
            run.task_id,
            run.id,
            run.finished_at
                .map(|at| at.to_rfc3339())
                .unwrap_or_else(|| "未知".into()),
            run.duration_ms
                .map(|ms| format!("{:.1} 秒", ms as f64 / 1000.0))
                .unwrap_or_else(|| "未知".into()),
            run.exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "无".into()),
        ),
        url: None,
    }];
    if let Some(url) = &event.run_url {
        fields.push(Field {
            label: "执行记录".into(),
            value: "查看执行记录".into(),
            url: Some(url.clone()),
        });
    }
    Message {
        title: format!("自动化失败 · {}", run.task_name),
        fields,
        body_label: "失败原因".into(),
        body: run
            .message
            .as_deref()
            .unwrap_or("未记录失败原因")
            .chars()
            .take(2000)
            .collect(),
        markdown: false,
        error: true,
    }
}
