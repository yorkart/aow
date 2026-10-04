use super::*;

impl Runtime {
    pub(in crate::runtime) fn mark_reaped(
        &self,
        status: TerminalPaneStatus,
        exit_code: Option<u32>,
        kill_remaining_session: bool,
    ) {
        // Serialize the post-wait session cleanup with DELETE's PID-based kill
        // path. Until this guard is released, DELETE cannot observe `reaped`
        // false and act on a PID that the OS may already be able to reuse.
        let Ok(mut reaped) = self.reap_state.lock() else {
            return;
        };
        if kill_remaining_session {
            self.kill_remaining_session_members();
        }
        if !self.deleted.load(Ordering::Acquire) {
            if let Ok(mut metadata) = self.metadata.lock() {
                metadata.status = status;
                metadata.exit_code = exit_code;
            }
            let _ = self.events.send(RuntimeEvent::Status { status, exit_code });
        }
        *reaped = true;
        self.reaped.notify_all();
    }

    pub(in crate::runtime) fn notify_deleted(&self) {
        if !self.deleted.swap(true, Ordering::AcqRel) {
            if let Some(vt_session) = &self.vt_session {
                vt_session.dispose();
            }
            self.deleted_changed.send_replace(true);
            let _ = self.events.send(RuntimeEvent::Deleted);
        }
    }

    pub(in crate::runtime) fn force_delete_and_reap(&self) -> Result<(), TerminaldError> {
        self.notify_deleted();
        self.kill_best_effort();
        let reaped = self
            .reap_state
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?;
        if *reaped {
            return Ok(());
        }
        let (reaped, timeout) = self
            .reaped
            .wait_timeout_while(reaped, DELETE_REAP_TIMEOUT, |reaped| !*reaped)
            .map_err(|_| TerminaldError::Poisoned)?;
        if timeout.timed_out() && !*reaped {
            return Err(TerminaldError::Worker(format!(
                "timed out reaping runtime {}",
                self.id
            )));
        }
        Ok(())
    }

    pub(in crate::runtime) fn kill_best_effort(&self) {
        if self.reap_state.lock().is_ok_and(|reaped| *reaped) {
            return;
        }
        let foreground_process_group = self
            .master
            .lock()
            .ok()
            .and_then(|master| master.process_group_leader());
        unsafe {
            if let Some(process_group) = foreground_process_group
                && process_group > 1
            {
                libc::kill(-process_group, libc::SIGKILL);
            }
            if let Some(pid) = self.child_pid
                && let Ok(pid) = i32::try_from(pid)
                && pid > 1
            {
                #[cfg(target_os = "linux")]
                if let Some(session_id) = self.child_session_id {
                    for member in linux_session_members(session_id) {
                        libc::kill(member, libc::SIGKILL);
                    }
                }
                #[cfg(target_os = "macos")]
                if let Some(session_id) = self.child_session_id {
                    macos_kill_session_members(session_id, self.child_start_time.as_deref());
                }
                libc::kill(-pid, libc::SIGKILL);
                libc::kill(pid, libc::SIGKILL);
            }
        }
        if let Ok(mut killer) = self.killer.lock() {
            let _ = killer.kill();
        }
    }

    pub(in crate::runtime) fn kill_uncommitted_spawn_best_effort(&self) {
        if self.reap_state.lock().is_ok_and(|reaped| *reaped) {
            return;
        }
        let foreground_process_group = self
            .master
            .lock()
            .ok()
            .and_then(|master| master.process_group_leader());
        unsafe {
            if let Some(process_group) = foreground_process_group
                && process_group > 1
            {
                libc::kill(-process_group, libc::SIGKILL);
            }
            if let Some(pid) = self.child_pid
                && let Ok(pid) = i32::try_from(pid)
                && pid > 1
            {
                libc::kill(-pid, libc::SIGKILL);
                // Start reaping the PTY leader immediately. Session enumeration
                // can be slow on macOS and is performed after wait() by
                // mark_reaped, which also removes any surviving descendants.
                libc::kill(pid, libc::SIGKILL);
            }
        }
        if let Ok(mut killer) = self.killer.lock() {
            let _ = killer.kill();
        }
    }

    pub(in crate::runtime) fn kill_remaining_session_members(&self) {
        let Some(session_id) = self.child_session_id else {
            return;
        };
        #[cfg(target_os = "linux")]
        unsafe {
            // The PTY child is the session leader. `child.wait()` has already
            // reaped it, so every remaining member is a descendant that must
            // not outlive the completed runtime.
            for member in linux_session_members(session_id) {
                libc::kill(member, libc::SIGKILL);
            }
        }
        #[cfg(target_os = "macos")]
        macos_kill_session_members(session_id, self.child_start_time.as_deref());
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        unsafe {
            // Portable best effort for Unix systems without /proc session
            // enumeration. This reaches jobs still in the leader's group.
            libc::kill(-session_id, libc::SIGKILL);
        }
    }
}
