use super::*;

impl Runtime {
    pub(in crate::runtime) fn write_input(
        &self,
        owner: ControllerOwner,
        bytes: &[u8],
    ) -> Result<bool, TerminaldError> {
        let controller = self
            .controller
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?;
        if controller.owner != Some(owner) {
            return Ok(false);
        }
        let mut writer = self.writer.lock().map_err(|_| TerminaldError::Poisoned)?;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(true)
    }

    pub(in crate::runtime) fn resize(
        &self,
        owner: ControllerOwner,
        rows: u16,
        cols: u16,
    ) -> Result<bool, TerminaldError> {
        validate_dimensions(rows, cols)?;
        let controller = self
            .controller
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?;
        if controller.owner != Some(owner) {
            return Ok(false);
        }
        {
            let metadata = self.metadata.lock().map_err(|_| TerminaldError::Poisoned)?;
            if metadata.status != TerminalPaneStatus::Running {
                return Err(TerminaldError::Conflict(self.id.clone()));
            }
        }
        // Output append uses this same lock, making the VT resize offset an
        // exact boundary relative to every queued write. Node RPC remains
        // asynchronous; no standard mutex is held while it runs.
        let output = self.output.lock().map_err(|_| TerminaldError::Poisoned)?;
        self.master
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TerminaldError::Pty(error.to_string()))?;
        if let Some(vt_session) = &self.vt_session {
            vt_session.resize(output.next_offset, cols, rows);
        }
        drop(output);
        let mut metadata = self.metadata.lock().map_err(|_| TerminaldError::Poisoned)?;
        metadata.rows = rows;
        metadata.cols = cols;
        Ok(true)
    }
}
