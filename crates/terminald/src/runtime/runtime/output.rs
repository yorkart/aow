use super::*;

impl Runtime {
    pub(in crate::runtime) fn append_output(&self, bytes: &[u8]) -> Result<(), TerminaldError> {
        let mut output = self.output.lock().map_err(|_| TerminaldError::Poisoned)?;
        {
            let OutputState {
                state_parser,
                terminal,
                ..
            } = &mut *output;
            state_parser.advance(terminal, bytes);
        }
        let offset = output.append(bytes, SCROLLBACK_LIMIT)?;
        if let Some(vt_session) = &self.vt_session {
            vt_session.write(offset, bytes);
        }
        let _ = self.events.send(RuntimeEvent::Output {
            offset,
            bytes: Bytes::copy_from_slice(bytes),
        });
        Ok(())
    }

    pub(in crate::runtime) fn close_output(&self) {
        if let Ok(mut output) = self.output.lock() {
            output.closed = true;
            let _ = self.events.send(RuntimeEvent::OutputClosed);
        }
    }

    pub(in crate::runtime) fn snapshot_and_subscribe(
        &self,
        after: u64,
    ) -> Result<(OutputSnapshot, broadcast::Receiver<RuntimeEvent>), TerminaldError> {
        // Output producers hold this same lock while broadcasting, so the
        // snapshot and replacement receiver form one atomic stream boundary.
        let mut output = self.output.lock().map_err(|_| TerminaldError::Poisoned)?;
        let events = self.events.subscribe();
        let snapshot = output.snapshot(Some(after));
        Ok((snapshot, events))
    }
}
