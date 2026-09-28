use super::*;

impl DaemonState {
    pub(in crate::runtime) async fn create(
        &self,
        id: String,
        spec: TerminalRuntimeSpec,
    ) -> Result<CreateOutcome, TerminaldError> {
        if self.inner.shutting_down.load(Ordering::Acquire) {
            return Err(TerminaldError::ShuttingDown);
        }
        validate_id(&id)?;

        let spawn_permit = {
            let runtimes = self.lock_runtimes()?;
            if self.inner.shutting_down.load(Ordering::Acquire) {
                return Err(TerminaldError::ShuttingDown);
            }
            if let Some(existing) = runtimes.get(&id).cloned() {
                return compare_existing(&id, existing, &spec);
            }
            // Register under the same map lock used by shutdown's drain. Once
            // shutdown has set its gate, no new permit can appear after its
            // final zero observation.
            self.inner.in_flight_spawns.begin()
        };
        // Existing IDs are resolved before filesystem validation so an
        // identical idempotent PUT keeps succeeding even if its cwd or shell
        // was renamed after the process started. A different spec is always a
        // conflict, rather than an unrelated validation error.
        validate_spec(&spec).await?;

        let spawn_spec = spec.clone();
        let spawn_id = id.clone();
        let vt_worker = self.inner.vt_worker.clone();
        let spawned = tokio::task::spawn_blocking(move || {
            spawn_runtime_blocking(spawn_id, spawn_spec, spawn_permit, vt_worker)
        })
        .await
        .map_err(|error| TerminaldError::Worker(error.to_string()))??;

        let mut runtimes = self.lock_runtimes()?;
        if self.inner.shutting_down.load(Ordering::Acquire) {
            drop(runtimes);
            drop(spawned);
            return Err(TerminaldError::ShuttingDown);
        }
        if let Some(existing) = runtimes.get(&id).cloned() {
            drop(runtimes);
            drop(spawned);
            return compare_existing(&id, existing, &spec);
        }
        let description = spawned.runtime.description()?;
        runtimes.insert(id, spawned.runtime.clone());
        drop(runtimes);
        start_runtime_workers(spawned);
        Ok(CreateOutcome::Created(description))
    }

    pub(in crate::runtime) async fn delete(&self, id: &str) -> Result<(), TerminaldError> {
        // Keep the map's owning reference until the process is confirmed
        // reaped. A timeout or canceled request can therefore retry DELETE
        // without orphaning the only lifecycle handle.
        let runtime = self.lock_runtimes()?.get(id).cloned();
        let Some(runtime) = runtime else {
            return Ok(());
        };
        runtime.notify_deleted();
        let state = self.clone();
        let id = id.to_owned();
        // This task intentionally outlives a canceled HTTP request. Once a
        // DELETE has marked a runtime, cleanup must either reap and remove it
        // or retain the tombstone in the map so a later DELETE can retry.
        tokio::spawn(async move {
            let delete_runtime = runtime.clone();
            tokio::task::spawn_blocking(move || delete_runtime.force_delete_and_reap())
                .await
                .map_err(|error| TerminaldError::Worker(error.to_string()))??;

            let mut runtimes = state.lock_runtimes()?;
            if runtimes
                .get(&id)
                .is_some_and(|current| Arc::ptr_eq(current, &runtime))
            {
                runtimes.remove(&id);
            }
            Ok(())
        })
        .await
        .map_err(|error| TerminaldError::Worker(error.to_string()))?
    }

    pub(in crate::runtime) async fn shutdown_all(&self) -> Result<(), TerminaldError> {
        let runtimes = {
            let mut state = self.lock_runtimes()?;
            // The gate transition shares this mutex with create's permit
            // registration and map commit, giving the shutdown boundary a
            // concrete happens-before relationship instead of relying on a
            // racing atomic observation.
            self.inner.shutting_down.store(true, Ordering::Release);
            state
                .drain()
                .map(|(_, runtime)| runtime)
                .collect::<Vec<_>>()
        };
        for runtime in &runtimes {
            runtime.notify_deleted();
            runtime.kill_best_effort();
        }
        let mut first_error = None;
        for runtime in runtimes {
            match tokio::task::spawn_blocking(move || runtime.force_delete_and_reap()).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    first_error.get_or_insert(error);
                }
                Err(error) => {
                    first_error.get_or_insert_with(|| TerminaldError::Worker(error.to_string()));
                }
            };
        }
        // Includes shells spawned by create requests that were canceled before
        // map commit. Their SpawnedRuntime guards retain a permit until the
        // detached cleanup thread has completed child.wait().
        self.inner.in_flight_spawns.wait_zero().await;
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for DaemonInner {
    fn drop(&mut self) {
        let Ok(runtimes) = self.runtimes.get_mut() else {
            return;
        };
        for runtime in runtimes.values() {
            runtime.notify_deleted();
            runtime.kill_best_effort();
        }
    }
}
