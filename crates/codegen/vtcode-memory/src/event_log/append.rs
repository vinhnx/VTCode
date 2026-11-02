//! Append path: event serialization, turn-boundary persistence, and cap eviction.
//!
//! Appends batch in memory until a turn boundary (or an explicit read/flush);
//! cap enforcement is the one append-time path that rewrites the on-disk file.

use super::*;

impl SessionEventLog {
    /// Append an event to the log and update the in-memory index/manifest.
    pub fn append(&self, event: &ThreadEvent) -> Result<(), SessionStoreError> {
        let _eviction_guard = self.shared.eviction_lock.lock().map_err(poison)?;
        let mut st = self.shared.state.lock().map_err(poison)?;

        // Serialize into the write buffer with rollback on failure — the
        // invariant that `write_buf` never contains partial JSON is
        // encapsulated in `serialize_event`.
        let (start, end) = st.serialize_event(event)?;

        st.manifest.event_count += 1;
        st.manifest.updated_at = now_rfc3339();

        // Route through the single turn-lifecycle state machine.  When the
        // event closes a turn, persist metadata immediately so a reopen
        // after a mid-turn crash sees a consistent index.
        let is_turn_boundary = st.apply_lifecycle_event(LifecycleKind::from_event(event), start, end);
        if is_turn_boundary {
            self.persist_meta_locked(&mut st)?;
        }

        if st.write_buf.len() >= MAX_WRITE_BUFFER_BYTES {
            // Persist metadata with the bounded byte flush so a reopen after
            // a mid-turn crash does not trust an index that predates these
            // already-written events.
            self.persist_meta_locked(&mut st)?;
        }
        drop(st);
        self.enforce_event_cap()
    }

    /// Enforce the per-session event cap by evicting the oldest completed
    /// turns when the log exceeds [`Self::max_events`]. Returns `Ok(())` even
    /// when no truncation is needed or the cap is disabled (`max_events == 0`).
    fn enforce_event_cap(&self) -> Result<(), SessionStoreError> {
        let mut st = self.shared.state.lock().map_err(poison)?;

        // `plan_cap_eviction` encapsulates the index arithmetic and returns
        // `None` when the cap is disabled or not yet exceeded.
        let Some(plan) = st.plan_cap_eviction(self.max_events) else {
            return Ok(());
        };

        // Keep ordinary appends in memory until a turn boundary or an
        // explicit read. Cap enforcement is the one append-time path that
        // needs the complete on-disk file before rewriting it.
        self.flush_write_buf_locked(&mut st)?;

        let (evicted, mut remaining, previous_file_len) = {
            let mut file_slot = self.shared.file.lock().map_err(poison)?;
            let file = file_slot.as_mut().ok_or_else(|| self.event_file_unavailable())?;
            let file_len = file
                .metadata()
                .map_err(|error| SessionStoreError::io(&self.events_path, error))?
                .len();
            if plan.truncate_offset > file_len {
                return Err(SessionStoreError::io(
                    &self.events_path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "cap offset exceeds event log length"),
                ));
            }
            file.seek(SeekFrom::Start(0))
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            let mut evicted = vec![
                0u8;
                usize::try_from(plan.truncate_offset).map_err(|error| {
                    SessionStoreError::io(
                        &self.events_path,
                        std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                    )
                })?
            ];
            file.read_exact(&mut evicted)
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            file.seek(SeekFrom::Start(plan.truncate_offset))
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            let mut remaining = Vec::new();
            file.read_to_end(&mut remaining)
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            (evicted, remaining, file_len)
        };
        let retained_turn_base = st.retained_turn_base_after_eviction(plan.evicted_turn_count);
        drop(st);

        // The turn index counts only records inside indexed turns. The bytes
        // removed by a cap rewrite may also contain valid session-level
        // records (for example `thread.started`) before the first turn, so
        // reconcile the manifest against the actual persisted prefix rather
        // than the turn-only estimate from `plan_cap_eviction`.
        let mut plan = plan;
        plan.evicted_event_count = count_persisted_event_records(&evicted);
        let evicted_events = decode_events(&evicted);
        (self.eviction_summary_hook)(&evicted_events)?;

        // Matrix snapshots are complete session-level checkpoints. Preserve the
        // latest evicted checkpoint only when no newer snapshot survives.
        let mut matrix_checkpoints = BTreeMap::new();
        let mut planning_checkpoint = None;
        for event in &evicted_events {
            if let ThreadEvent::PlanDelta(delta) = event
                && delta.recovery.is_some()
            {
                planning_checkpoint = Some(event);
            }
            if let ThreadEvent::MatrixUpdated(snapshot) = event {
                matrix_checkpoints.insert(snapshot.spec.id.clone(), event);
            }
        }
        for event in decode_events(&remaining) {
            if let ThreadEvent::PlanDelta(delta) = &event
                && delta.recovery.is_some()
            {
                planning_checkpoint = None;
            }
            if let ThreadEvent::MatrixUpdated(snapshot) = event {
                matrix_checkpoints.remove(&snapshot.spec.id);
            }
        }
        let mut checkpoint_prefix = Vec::new();
        for event in matrix_checkpoints.values().copied().chain(planning_checkpoint) {
            serde_json::to_writer(&mut checkpoint_prefix, &VersionedThreadEvent::new((*event).clone()))
                .map_err(|error| SessionStoreError::io(&self.events_path, std::io::Error::other(error)))?;
            checkpoint_prefix.push(b'\n');
        }
        let checkpoint_bytes = checkpoint_prefix.len() as u64;
        let checkpoint_count = matrix_checkpoints.len() as u64 + u64::from(planning_checkpoint.is_some());
        if !checkpoint_prefix.is_empty() {
            checkpoint_prefix.extend_from_slice(&remaining);
            remaining = checkpoint_prefix;
        }

        let new_file_len = u64::try_from(remaining.len()).map_err(|error| {
            SessionStoreError::io(&self.events_path, std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })?;
        self.manifest_store.write_pending_cap_rewrite(&PendingCapRewrite {
            previous_file_len,
            new_file_len,
            retained_turn_base,
        })?;
        let next_offset = self.replace_event_file_contents(&remaining)?;
        let mut st = self.shared.state.lock().map_err(poison)?;
        st.apply_cap_eviction(plan, next_offset);
        for entry in &mut st.index.entries {
            entry.start_offset += checkpoint_bytes;
            entry.end_offset += checkpoint_bytes;
        }
        st.manifest.event_count += checkpoint_count;
        // The rewrite changed byte offsets and retained counts; persist the
        // derived metadata before exposing the append as successful.
        self.persist_meta_locked(&mut st)?;
        self.manifest_store.clear_pending_cap_rewrite()?;
        Ok(())
    }
}
