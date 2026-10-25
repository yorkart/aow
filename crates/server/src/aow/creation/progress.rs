use super::*;
use crate::operations::Handle;
use aow_operation_log::{Level, Outcome};
use std::{
    future::Future,
    time::{Duration, Instant},
};

pub(super) struct Progress {
    pub(super) manager: AowManager,
    pub(super) id: String,
    pub(super) operation: Handle,
    pub(super) timeout: Duration,
}

impl Progress {
    fn update(&self, change: impl FnOnce(&mut CreationJob)) {
        if let Ok(mut jobs) = self.manager.inner.creations.lock()
            && let Some(job) = jobs.iter_mut().find(|job| job.id == self.id)
            && job.status.active()
        {
            change(job);
        }
    }

    pub(super) fn skip(&self, index: usize) {
        self.update(|job| {
            job.steps[index].status = Status::Skipped;
            job.steps[index].message =
                Some(aow_i18n::text("server.main_repository_update_was_not_selected").into());
        });
        self.operation.event(
            Level::Info,
            aow_i18n::text("server.main_repository_update_skipped"),
            Some(index + 1),
        );
    }

    pub(super) fn finish(&self, status: Status, message: String, path: Option<String>) {
        self.update(|job| {
            job.status = status;
            job.finished = Some(Instant::now());
            if let Some(path) = path {
                job.path = path;
            }
            if status != Status::Succeeded {
                job.error = Some(message.clone());
            }
            for step in &mut job.steps {
                if step.status == Status::Running {
                    step.status = status;
                    step.duration_ms = step.started.map(|at| at.elapsed().as_millis() as u64);
                    step.message = Some(message.clone());
                } else if step.status == Status::Pending {
                    step.status = Status::Skipped;
                    step.message = Some(
                        aow_i18n::text(
                            "server.not_executed_because_a_previous_step_did_not_finish",
                        )
                        .into(),
                    );
                }
            }
        });
        self.operation.finish(
            match status {
                Status::Succeeded => Outcome::Succeeded,
                Status::Interrupted => Outcome::Interrupted,
                _ => Outcome::Failed,
            },
            message,
        );
    }
}

pub(super) async fn step<T>(
    progress: Option<&Progress>,
    index: usize,
    future: impl Future<Output = Result<T, AowError>>,
) -> Result<T, AowError> {
    let title = aow_i18n::text(model::STEP_TITLES[index]);
    let limit = progress.map_or(model::STEP_TIMEOUT, |progress| progress.timeout);
    if let Some(progress) = progress {
        progress.update(|job| {
            job.status = Status::Running;
            let step = &mut job.steps[index];
            step.status = Status::Running;
            step.started_at = Some(chrono::Utc::now().to_rfc3339());
            step.started = Some(Instant::now());
            step.timeout_ms = limit.as_millis() as u64;
        });
        progress.operation.progress(
            aow_i18n::message(
                "server.timeout_seconds",
                &[
                    ("p0", title.to_string()),
                    ("auto0", format!("{}", limit.as_secs())),
                ],
            ),
            Some(index),
        );
    }
    let started = Instant::now();
    let result = tokio::time::timeout(limit, future)
        .await
        .unwrap_or_else(|_| {
            Err(AowError::Timeout(aow_i18n::message(
                "server.timed_out_seconds_execution_stopped",
                &[
                    ("p0", title.to_string()),
                    ("auto0", format!("{}", limit.as_secs())),
                ],
            )))
        });
    if let Some(progress) = progress {
        let status = match &result {
            Ok(_) => Status::Succeeded,
            Err(AowError::Timeout(_)) => Status::TimedOut,
            Err(_) => Status::Failed,
        };
        let message = match &result {
            Ok(_) => aow_i18n::message(
                "server.completed_duration_seconds",
                &[
                    ("p0", title.to_string()),
                    ("auto0", format!("{:.1}", started.elapsed().as_secs_f64())),
                ],
            ),
            Err(error) => aow_i18n::message(
                "server.duration_seconds",
                &[
                    ("p0", title.to_string()),
                    (
                        "auto0",
                        (if status == Status::TimedOut {
                            aow_i18n::text("server.timed_out")
                        } else {
                            aow_i18n::text("server.failed")
                        })
                        .to_string(),
                    ),
                    ("auto1", format!("{:.1}", started.elapsed().as_secs_f64())),
                    ("p3", format!("{}", error)),
                    (
                        "auto2",
                        (if index >= 3 {
                            aow_i18n::text(
                                "server.a_branch_or_directory_may_already_exist_refresh_projects",
                            )
                        } else {
                            ""
                        })
                        .to_string(),
                    ),
                ],
            ),
        };
        progress.update(|job| {
            let step = &mut job.steps[index];
            step.status = status;
            step.duration_ms = Some(started.elapsed().as_millis() as u64);
            step.message = Some(message.clone());
        });
        progress.operation.event(
            if result.is_ok() {
                Level::Info
            } else {
                Level::Error
            },
            message.clone(),
            Some(if result.is_ok() { index + 1 } else { index }),
        );
        if result.is_err() {
            progress.finish(status, message, None);
        }
    }
    result
}
