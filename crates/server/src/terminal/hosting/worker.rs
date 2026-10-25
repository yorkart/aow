use super::super::agent_control::connection::AgentConnection;
use super::super::notifications::TaskStopNotification;
use super::state::{elapsed, transition};
use super::*;
use anyhow::{Context, Result, ensure};
use aow_automations::RunStatus;
use tokio::sync::broadcast;

pub(crate) fn recover(state: AppState) {
    let Ok(tabs) = state.terminals.list_snapshot(None) else {
        return;
    };
    for pane in tabs.tabs.into_iter().flat_map(|tab| tab.panes) {
        if let Some(hosting) = pane.hosting {
            let completions = state.terminals.subscribe_task_completions();
            spawn(state.clone(), pane.id, hosting, None, completions, false);
        }
    }
}

pub(super) fn spawn(
    state: AppState,
    pane_id: String,
    hosting: TerminalHosting,
    connection: Option<AgentConnection>,
    completions: broadcast::Receiver<TaskStopNotification>,
    run_on_enable: bool,
) {
    if hosting.phase.stopped() {
        return;
    }
    let Ok(mut workers) = state.terminals.inner.hosting_workers.lock() else {
        return;
    };
    if !workers.insert(hosting.id.clone()) {
        return;
    }
    drop(workers);
    aow_i18n::spawn(async move {
        if let Err(error) = run(
            &state,
            &pane_id,
            &hosting.id,
            connection,
            completions,
            run_on_enable,
        )
        .await
            && let Ok(gate) = state.terminals.hosting_gate(&pane_id)
        {
            let _gate = gate.lock().await;
            if let Ok(Some(mut current)) = state.terminals.hosting(&pane_id)
                && current.id == hosting.id
                && !current.phase.stopped()
            {
                transition(&mut current, TerminalHostingPhase::Failed);
                current.error = Some(format!("{error:#}"));
                if let Err(error) =
                    state
                        .terminals
                        .set_hosting(&pane_id, Some(&hosting.id), Some(current))
                {
                    tracing::error!(%error, %pane_id, "cannot persist hosting failure");
                }
                state.workspace_events.terminals_changed();
            }
        }
        if let Ok(mut workers) = state.terminals.inner.hosting_workers.lock() {
            workers.remove(&hosting.id);
        }
    });
}

async fn run(
    state: &AppState,
    pane_id: &str,
    id: &str,
    connection: Option<AgentConnection>,
    mut completions: broadcast::Receiver<TaskStopNotification>,
    run_on_enable: bool,
) -> Result<()> {
    let manager = &state.terminals;
    let gate = manager.hosting_gate(pane_id)?;
    let mut connection = match connection {
        Some(connection) => connection,
        None => {
            let _gate = gate.lock().await;
            let Some(current) = manager.hosting(pane_id)?.filter(|hosting| hosting.id == id) else {
                return Ok(());
            };
            if current.phase.stopped() {
                return Ok(());
            }
            ensure!(
                current.phase != TerminalHostingPhase::Submitting,
                aow_i18n::message(
                    "server.aow_restarted_while_submitting_feedback_delivery_status_is_unknown",
                    &[]
                )
            );
            let source = source::read(manager, pane_id).await?;
            ensure!(
                source::matches(&current, &source),
                aow_i18n::message(
                    "server.agent_process_or_session_changed_take_control_and_reconfigure",
                    &[]
                )
            );
            tokio::time::timeout(
                Duration::from_secs(10),
                AgentConnection::claim_with_force(&manager.inner.terminald, pane_id, true),
            )
            .await
            .context(aow_i18n::text(
                "server.timed_out_restoring_autopilot_control",
            ))??
        }
    };
    // Only a new enable passes this flag. Persist the first run before polling
    // completions so concurrent events cannot start another review. Recovery
    // follows any persisted run but never retries an interrupted initial start.
    if run_on_enable {
        let Some(hosting) = manager.hosting(pane_id)?.filter(|hosting| hosting.id == id) else {
            return Ok(());
        };
        if hosting.phase == TerminalHostingPhase::Waiting {
            start_review(state, pane_id, hosting, None).await?;
        }
    }
    let mut interval = tokio::time::interval(Duration::from_millis(1500));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let Some(hosting) = manager.hosting(pane_id)?.filter(|hosting| hosting.id == id) else {
            return Ok(());
        };
        if hosting.phase.stopped() {
            return Ok(());
        }
        tokio::select! {
            message = connection.socket.next() => {
                if let Some(aow_protocol::TerminalAttachServerMessage::Control { state }) = AgentConnection::check(message)? {
                    ensure!(state == aow_protocol::TerminalControlState::Claimed, aow_i18n::message("server.autopilot_control_lost_take_control_and_retry", &[]));
                }
            }
            event = completions.recv() => {
                let event = event.context(aow_i18n::text("server.task_completion_event_subscription_interrupted_or_events_lost_due"))?;
                if accepts(&hosting, pane_id, &event) {
                    start_review(state, pane_id, hosting, Some(event)).await?;
                }
            }
            _ = interval.tick() => {
                ensure!(sessions::same_process(&hosting.process), aow_i18n::message("server.agent_process_changed_take_control_and_reconfigure_autopilot", &[]));
                step(state, pane_id, hosting, &mut completions, &mut connection).await?;
            }
        }
    }
}

pub(super) fn accepts(
    hosting: &TerminalHosting,
    pane_id: &str,
    event: &TaskStopNotification,
) -> bool {
    hosting.phase == TerminalHostingPhase::Waiting
        && event.agent == hosting.agent
        && event.session_id == hosting.session_id
        && event.instance_ids.iter().any(|id| id == pane_id)
        && event
            .turn_id
            .as_ref()
            .is_none_or(|id| hosting.source_turn_id.as_ref() != Some(id))
}

async fn start_review(
    state: &AppState,
    pane_id: &str,
    mut hosting: TerminalHosting,
    event: Option<TaskStopNotification>,
) -> Result<()> {
    let source = source::read(&state.terminals, pane_id).await?;
    ensure!(
        source::matches(&hosting, &source),
        aow_i18n::message(
            "server.agent_process_or_session_changed_take_control_and_reconfigure",
            &[]
        )
    );
    let automation = state
        .automations
        .as_ref()
        .context(aow_i18n::text("server.automation_service_unavailable"))?;
    let task = automation.hosting_task(&hosting.task_id, hosting.task_revision)?;
    hosting.source_turn_id = event.as_ref().and_then(|event| event.turn_id.clone());
    hosting.run_id = Some(aow_automations::store::new_run_id());
    transition(&mut hosting, TerminalHostingPhase::Reviewing);
    save(state, pane_id, &hosting)?;
    automation
        .dispatch_hosted(task, &hosting, event.as_ref())
        .await
}

async fn step(
    state: &AppState,
    pane_id: &str,
    mut hosting: TerminalHosting,
    completions: &mut broadcast::Receiver<TaskStopNotification>,
    connection: &mut AgentConnection,
) -> Result<()> {
    let automation = state
        .automations
        .as_ref()
        .context(aow_i18n::text("server.automation_service_unavailable"))?;
    match hosting.phase {
        TerminalHostingPhase::Waiting => {}
        TerminalHostingPhase::Reviewing => {
            let run_id = hosting
                .run_id
                .as_deref()
                .context(aow_i18n::text("server.run_id_missing"))?;
            let Some(run) = automation.hosted_run(&hosting.task_id, run_id)? else {
                ensure!(
                    elapsed(&hosting) < 90,
                    aow_i18n::message(
                        "server.runner_startup_timed_out_no_run_record_created_yet",
                        &[]
                    )
                );
                return Ok(());
            };
            if !run.status.terminal() {
                return Ok(());
            }
            ensure!(
                run.status == RunStatus::Completed,
                "{}",
                run.message.as_deref().unwrap_or(aow_i18n::text(
                    "server.autopilot_task_did_not_complete_normally"
                ))
            );
            transition(&mut hosting, TerminalHostingPhase::Collecting);
            save(state, pane_id, &hosting)?;
        }
        TerminalHostingPhase::Collecting => {
            let run = automation
                .hosted_run(
                    &hosting.task_id,
                    hosting
                        .run_id
                        .as_deref()
                        .context(aow_i18n::text("server.run_id_missing"))?,
                )?
                .context(aow_i18n::text("server.run_record_lost"))?;
            let result = automation.run_result(&run)?;
            let source = source::read(&state.terminals, pane_id).await?;
            // Slow scheduler/history I/O never delays a human takeover. Only
            // the final ownership check and paste + Enter hold the input gate.
            let gate = state.terminals.hosting_gate(pane_id)?;
            let _gate = gate.lock().await;
            if state
                .terminals
                .hosting(pane_id)?
                .is_none_or(|current| current.id != hosting.id)
            {
                return Ok(());
            }
            ensure!(
                sessions::same_process(&hosting.process),
                aow_i18n::message(
                    "server.original_agent_process_changed_autopilot_conclusion_not_submitted",
                    &[]
                )
            );
            ensure!(
                source::matches(&hosting, &source),
                aow_i18n::message(
                    "server.original_agent_process_or_session_changed_autopilot_conclusion_not",
                    &[]
                )
            );
            // A passing final review still wins when all feedback slots are used.
            // Keep the outcome for observers and stop without submitting any text.
            if aow_automations::hosting::review_passed(&result) {
                transition(&mut hosting, TerminalHostingPhase::Completed);
                return save(state, pane_id, &hosting);
            }
            if hosting.input_count >= hosting.max_inputs {
                transition(&mut hosting, TerminalHostingPhase::LimitReached);
                return save(state, pane_id, &hosting);
            }
            super::super::agent_control::api::validate_task(&result)?;
            transition(&mut hosting, TerminalHostingPhase::Submitting);
            save(state, pane_id, &hosting)?;
            // Discard events received during review, but retain any completion
            // emitted while this submission is being acknowledged.
            *completions = completions.resubscribe();
            tokio::time::timeout(Duration::from_secs(15), connection.submit(&result))
                .await
                .context(aow_i18n::text(
                    "server.feedback_submission_timed_out_delivery_status_is_unknown_take",
                ))??;
            hosting.input_count += 1;
            transition(&mut hosting, TerminalHostingPhase::Waiting);
            save(state, pane_id, &hosting)?;
        }
        TerminalHostingPhase::Submitting => {
            anyhow::bail!(aow_i18n::message(
                "server.feedback_delivery_status_unknown_take_control_to_confirm_no",
                &[]
            ))
        }
        TerminalHostingPhase::Completed
        | TerminalHostingPhase::LimitReached
        | TerminalHostingPhase::Failed => {}
    }
    Ok(())
}

fn save(state: &AppState, pane_id: &str, hosting: &TerminalHosting) -> Result<()> {
    ensure!(
        state
            .terminals
            .set_hosting(pane_id, Some(&hosting.id), Some(hosting.clone()))?,
        aow_i18n::message("server.autopilot_disabled", &[])
    );
    state.workspace_events.terminals_changed();
    Ok(())
}
