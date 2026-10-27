use super::*;

// A window holds many PTY reads and one coalesced VT write. Waiting releases
// the output lock, so ACK publication, resize and deletion remain available.
const PENDING_OUTPUT_LIMIT: usize = 64 * 1024;

impl Runtime {
    pub(in crate::runtime) fn append_output(&self, bytes: &[u8]) -> Result<(), TerminaldError> {
        for bytes in bytes.chunks(PENDING_OUTPUT_LIMIT) {
            let mut output = self.output.lock().map_err(|_| TerminaldError::Poisoned)?;
            if let Some(vt_session) = &self.vt_session {
                output = self
                    .output_capacity
                    .wait_while(output, |output| {
                        output.pending.bytes.len() + bytes.len() > PENDING_OUTPUT_LIMIT
                            && output.pending.error.is_none()
                            && !self.is_deleted()
                    })
                    .map_err(|_| TerminaldError::Poisoned)?;
                output.ensure_available()?;
                if self.is_deleted() {
                    return Err(TerminaldError::NotFound(self.id.clone()));
                }
                let offset = output.received_offset();
                output.pending.bytes.extend(bytes);
                vt_session.write(offset, bytes);
                self.output_changed.notify_one();
            } else {
                self.commit_output(&mut output, bytes)?;
            }
        }
        Ok(())
    }

    fn commit_output(&self, output: &mut OutputState, bytes: &[u8]) -> Result<(), TerminaldError> {
        let OutputState {
            state_parser,
            terminal,
            ..
        } = output;
        state_parser.advance(terminal, bytes);
        let offset = output.append(bytes, SCROLLBACK_LIMIT)?;
        let _ = self.events.send(RuntimeEvent::Output {
            offset,
            bytes: Bytes::copy_from_slice(bytes),
        });
        Ok(())
    }

    pub(in crate::runtime) fn close_output(&self) {
        if let Ok(mut output) = self.output.lock() {
            output.pending.eof = true;
            if self.vt_session.is_none() {
                output.closed = true;
                let _ = self.events.send(RuntimeEvent::OutputClosed);
            }
            self.output_changed.notify_one();
        }
    }

    pub(in crate::runtime) async fn publish_vt_output(&self) {
        let Some(session) = &self.vt_session else {
            return;
        };
        let mut applied = session.applied_offsets();
        let mut deleted = self.deleted_changed.subscribe();
        let mut deadline = tokio::time::Instant::now() + session.request_timeout();
        let mut was_pending = false;
        let mut last_offset = None;
        loop {
            if self.is_deleted() {
                return;
            }
            let offset = *applied.borrow_and_update();
            let pending = match self.publish_applied_output(offset) {
                Ok(None) => return,
                Ok(Some(pending)) => pending,
                Err(error) => {
                    session.fail();
                    self.fail_output(error.to_string());
                    return;
                }
            };
            // New PTY bytes must not extend a stalled ACK's deadline. Only
            // actual parse progress (or a newly nonempty window) restarts it.
            if pending && (!was_pending || offset != last_offset) {
                deadline = tokio::time::Instant::now() + session.request_timeout();
            }
            was_pending = pending;
            last_offset = offset;
            tokio::select! {
                biased;
                _ = deleted.changed() => return,
                changed = applied.changed() => {
                    if changed.is_err() {
                        self.fail_output("VT worker acknowledgement stream closed".to_owned());
                        return;
                    }
                }
                _ = self.output_changed.notified() => {}
                _ = tokio::time::sleep_until(deadline), if pending => {
                    session.fail();
                    self.fail_output("VT worker output acknowledgement timed out".to_owned());
                    return;
                }
            }
        }
    }

    // Only committed bytes enter scrollback, replay and broadcast. Initial
    // attachment and lag recovery therefore obey the same ACK boundary.
    // None means EOF has been committed; Some reports whether an ACK is due.
    fn publish_applied_output(&self, applied: Option<u64>) -> Result<Option<bool>, TerminaldError> {
        let mut output = self.output.lock().map_err(|_| TerminaldError::Poisoned)?;
        let applied = applied.ok_or_else(|| {
            TerminaldError::Worker("VT worker unavailable; terminal output stopped".to_owned())
        })?;
        if !(output.next_offset..=output.received_offset()).contains(&applied) {
            return Err(TerminaldError::Worker(
                "invalid VT output acknowledgement offset".to_owned(),
            ));
        }
        let count = (applied - output.next_offset) as usize;
        if count != 0 {
            let bytes = output.pending.bytes.drain(..count).collect::<Vec<_>>();
            self.commit_output(&mut output, &bytes)?;
            self.output_capacity.notify_all();
        }
        if output.pending.eof && output.pending.bytes.is_empty() {
            output.closed = true;
            let _ = self.events.send(RuntimeEvent::OutputClosed);
            return Ok(None);
        }
        Ok(Some(!output.pending.bytes.is_empty()))
    }

    fn fail_output(&self, message: String) {
        if let Ok(mut output) = self.output.lock()
            && output.pending.error.is_none()
        {
            output.pending.error = Some(message.clone());
            let _ = self.events.send(RuntimeEvent::OutputFailed(message));
        }
        self.output_capacity.notify_all();
    }

    pub(in crate::runtime) fn snapshot_and_subscribe(
        &self,
        after: u64,
    ) -> Result<(OutputSnapshot, broadcast::Receiver<RuntimeEvent>), TerminaldError> {
        // Publication holds this lock while broadcasting, so the snapshot and
        // replacement receiver form one atomic, parsed stream boundary.
        let mut output = self.output.lock().map_err(|_| TerminaldError::Poisoned)?;
        output.ensure_available()?;
        let events = self.events.subscribe();
        let snapshot = output.snapshot(Some(after));
        Ok((snapshot, events))
    }
}
