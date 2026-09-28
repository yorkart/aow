use super::*;

impl DaemonState {
    pub(in crate::runtime) fn new() -> Self {
        Self::with_vt_worker(None)
    }

    pub(in crate::runtime) fn with_vt_worker(vt_worker: Option<VtWorkerClient>) -> Self {
        Self {
            inner: Arc::new(DaemonInner {
                instance_id: Uuid::new_v4().to_string(),
                shutting_down: AtomicBool::new(false),
                in_flight_spawns: Arc::new(SpawnTracker::default()),
                runtimes: Mutex::new(HashMap::new()),
                vt_worker,
                agent_cache: Mutex::new(agents::AgentCache::default()),
            }),
        }
    }

    pub(in crate::runtime) fn lock_runtimes(
        &self,
    ) -> Result<MutexGuard<'_, HashMap<String, Arc<Runtime>>>, TerminaldError> {
        self.inner
            .runtimes
            .lock()
            .map_err(|_| TerminaldError::Poisoned)
    }

    pub(in crate::runtime) fn health(&self) -> TerminaldHealth {
        TerminaldHealth {
            service: SERVICE_NAME.to_owned(),
            instance_id: self.inner.instance_id.clone(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            pid: cfg!(target_os = "macos").then(std::process::id),
        }
    }

    pub(in crate::runtime) fn list(&self) -> Result<TerminalRuntimeList, TerminaldError> {
        let runtimes = self.lock_runtimes()?;
        let mut descriptions = runtimes
            .values()
            .filter(|runtime| !runtime.is_deleted())
            .map(|runtime| runtime.description())
            .collect::<Result<Vec<_>, _>>()?;
        descriptions.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(TerminalRuntimeList {
            runtimes: descriptions,
        })
    }

    pub(in crate::runtime) fn get(&self, id: &str) -> Result<TerminalRuntime, TerminaldError> {
        let runtime = self
            .lock_runtimes()?
            .get(id)
            .cloned()
            .ok_or_else(|| TerminaldError::NotFound(id.to_owned()))?;
        if runtime.is_deleted() {
            return Err(TerminaldError::NotFound(id.to_owned()));
        }
        runtime.description()
    }

    pub(in crate::runtime) fn screen(
        &self,
        id: &str,
    ) -> Result<Option<aow_protocol::TerminalScreen>, TerminaldError> {
        let runtime = self
            .lock_runtimes()?
            .get(id)
            .cloned()
            .ok_or_else(|| TerminaldError::NotFound(id.into()))?;
        if runtime.is_deleted() {
            return Err(TerminaldError::NotFound(id.into()));
        }
        Ok(runtime
            .vt_session
            .as_ref()
            .and_then(VtSession::snapshot)
            .map(|snapshot| aow_protocol::TerminalScreen {
                generation: snapshot.generation,
                applied_offset: snapshot.applied_offset,
                cols: snapshot.cols,
                rows: snapshot.rows,
                lines: snapshot.lines,
            }))
    }

    pub(in crate::runtime) fn attach(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
        vt_snapshot: bool,
    ) -> Result<RuntimeConnection, TerminaldError> {
        let runtime = self
            .lock_runtimes()?
            .get(id)
            .cloned()
            .ok_or_else(|| TerminaldError::NotFound(id.to_owned()))?;
        runtime.connection(
            (epoch == Some(runtime.stream_epoch.as_str()))
                .then_some(after)
                .flatten(),
            vt_snapshot,
        )
    }
}
