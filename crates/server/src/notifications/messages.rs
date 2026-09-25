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
