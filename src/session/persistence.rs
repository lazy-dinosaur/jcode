use anyhow::Result;
use chrono::Utc;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use super::journal::{
    PersistVectorMode, SessionJournalEntry, metadata_requires_journal_append,
    metadata_requires_snapshot,
};
use super::storage_paths::{file_len_or_zero, session_journal_path_from_snapshot, session_path};
use super::{MAX_SESSION_JOURNAL_BYTES, RemoteStartupSessionSnapshot, Session, SessionStartupStub};
use crate::storage;

impl Session {
    fn apply_journal_entry(&mut self, entry: SessionJournalEntry) {
        self.apply_journal_meta(entry.meta);
        self.messages.extend(entry.append_messages);
        self.env_snapshots.extend(entry.append_env_snapshots);
        self.memory_injections
            .extend(entry.append_memory_injections);
        self.replay_events.extend(entry.append_replay_events);
        self.mark_memory_profile_dirty();
    }

    /// Replay journal entries from disk into this session, returning the
    /// number of entries successfully applied. Stops at first parse error
    /// (logging the failure) so partial recovery is still possible.
    ///
    /// Used by all session-load paths so journal-only data (messages
    /// appended after the last snapshot checkpoint) is never silently
    /// dropped on read. This is critical when the server is killed or
    /// crashes before `/save` runs — the snapshot file may be stale but
    /// the journal still has the latest user/assistant messages.
    fn replay_journal_from_path(&mut self, journal_path: &Path) -> std::io::Result<usize> {
        if !journal_path.exists() {
            return Ok(0);
        }
        let file = std::fs::File::open(journal_path)?;
        let reader = BufReader::new(file);
        let mut applied = 0usize;
        for (line_idx, line) in reader.lines().enumerate() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<SessionJournalEntry>(trimmed) {
                Ok(entry) => {
                    applied += 1;
                    self.apply_journal_entry(entry);
                }
                Err(err) => {
                    crate::logging::warn(&format!(
                        "Session journal parse failed at {} line {}: {} (stopping; {} entries applied)",
                        journal_path.display(),
                        line_idx + 1,
                        err,
                        applied
                    ));
                    break;
                }
            }
        }
        Ok(applied)
    }

    fn checkpoint_snapshot(&mut self, snapshot_path: &Path, journal_path: &Path) -> Result<()> {
        storage::write_json_fast(snapshot_path, self)?;
        if journal_path.exists() {
            let _ = std::fs::remove_file(journal_path);
        }
        self.reset_persist_state(true);
        Ok(())
    }

    /// Append a single new message to the session journal immediately.
    ///
    /// Best-effort: failure must not break the in-memory session, only logged.
    /// Caller must have already pushed `message` into `self.messages`.
    /// On success, advances `persist_state.messages_len` so the next `save()`
    /// does not duplicate this message in the save-time journal delta.
    pub(crate) fn append_journal_entry_for_new_message(
        &mut self,
        message: &super::StoredMessage,
    ) -> std::io::Result<()> {
        if self.persist_state.messages_mode == PersistVectorMode::Full {
            return Ok(());
        }

        let snapshot_path =
            session_path(&self.id).map_err(|err| std::io::Error::other(err.to_string()))?;
        if !snapshot_path.exists() {
            return Ok(());
        }

        let current_meta = self.journal_meta();
        if self
            .persist_state
            .last_meta
            .as_ref()
            .is_some_and(|prev| metadata_requires_snapshot(prev, &current_meta))
        {
            return Ok(());
        }

        let journal_path = session_journal_path_from_snapshot(&snapshot_path);
        let entry = SessionJournalEntry {
            meta: current_meta,
            append_messages: vec![message.clone()],
            append_env_snapshots: Vec::new(),
            append_memory_injections: Vec::new(),
            append_replay_events: Vec::new(),
        };

        match storage::append_json_line_fast(&journal_path, &entry) {
            Ok(()) => {
                self.persist_state.messages_len += 1;
                Ok(())
            }
            Err(err) => {
                let io_err = std::io::Error::other(err.to_string());
                crate::logging::warn(&format!(
                    "Immediate session journal append failed for {}: {}",
                    self.id, io_err
                ));
                Err(io_err)
            }
        }
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        let load_start = Instant::now();
        let snapshot_bytes = file_len_or_zero(path);
        let snapshot_start = Instant::now();
        let mut session: Session = storage::read_json(path)?;
        let snapshot_ms = snapshot_start.elapsed().as_millis();
        let journal_path = session_journal_path_from_snapshot(path);
        let journal_bytes = file_len_or_zero(&journal_path);
        let journal_start = Instant::now();
        let journal_entries = session.replay_journal_from_path(&journal_path)?;
        let journal_ms = journal_start.elapsed().as_millis();
        let finalize_start = Instant::now();
        session.reset_persist_state(path.exists());
        session.reset_provider_messages_cache();
        session.mark_memory_profile_dirty();
        // M48-C1: synthesize a `StoredCompactionTurn` for legacy sessions that
        // only have the old `compaction` field, so downstream code can treat
        // both schemas uniformly. The legacy field is preserved untouched.
        session.backfill_compaction_turns_from_legacy();
        let finalize_ms = finalize_start.elapsed().as_millis();
        crate::logging::info(&format!(
            "[TIMING] session_load: session={}, snapshot={}ms, journal={}ms, finalize={}ms, snapshot_bytes={}, journal_bytes={}, journal_entries={}, messages={}, env_snapshots={}, replay_events={}, total={}ms",
            session.id,
            snapshot_ms,
            journal_ms,
            finalize_ms,
            snapshot_bytes,
            journal_bytes,
            journal_entries,
            session.messages.len(),
            session.env_snapshots.len(),
            session.replay_events.len(),
            load_start.elapsed().as_millis(),
        ));
        Ok(session)
    }

    pub fn load(session_id: &str) -> Result<Self> {
        let path = session_path(session_id)?;
        Self::load_from_path(&path)
    }

    /// Load metadata needed for remote-client startup, plus any journal-only
    /// messages so that messages appended after the last snapshot checkpoint
    /// are not silently dropped on read.
    ///
    /// This is the safety-net fallback when `load_for_remote_startup` fails
    /// (e.g. snapshot file is corrupt, contains an unknown variant, or was
    /// truncated). Even in that case we want to surface whatever the journal
    /// holds rather than show the user an empty conversation.
    ///
    /// Lighter than `load_for_remote_startup`: skips heavyweight transcript
    /// vectors in the snapshot (only stub fields are decoded), so the remote
    /// client can paint quickly while the server performs the authoritative
    /// session restore + history bootstrap.
    pub fn load_startup_stub(session_id: &str) -> Result<Self> {
        let path = session_path(session_id)?;
        let reader = BufReader::new(std::fs::File::open(&path)?);
        let stub: SessionStartupStub = serde_json::from_reader(reader)?;
        let mut session = Self::session_from_startup_stub(stub);
        let journal_path = session_journal_path_from_snapshot(&path);
        let journal_entries = match session.replay_journal_from_path(&journal_path) {
            Ok(n) => n,
            Err(err) => {
                crate::logging::warn(&format!(
                    "load_startup_stub: journal replay failed for {}: {}",
                    session.id, err
                ));
                0
            }
        };
        if journal_entries > 0 {
            crate::logging::info(&format!(
                "load_startup_stub: session={}, journal_entries={}, messages={}",
                session.id,
                journal_entries,
                session.messages.len(),
            ));
        }
        session.reset_persist_state(path.exists());
        session.reset_provider_messages_cache();
        session.mark_memory_profile_dirty();
        Ok(session)
    }

    pub fn load_for_remote_startup(session_id: &str) -> Result<Self> {
        let path = session_path(session_id)?;
        let load_start = Instant::now();
        let snapshot_bytes = file_len_or_zero(&path);
        let snapshot_start = Instant::now();
        let reader = BufReader::new(std::fs::File::open(&path)?);
        let snapshot: RemoteStartupSessionSnapshot = serde_json::from_reader(reader)?;
        let snapshot_ms = snapshot_start.elapsed().as_millis();
        let mut session = Self::session_from_remote_startup_snapshot(snapshot);
        let journal_path = session_journal_path_from_snapshot(&path);
        let journal_bytes = file_len_or_zero(&journal_path);
        let journal_start = Instant::now();
        let journal_entries = session.replay_journal_from_path(&journal_path)?;
        let journal_ms = journal_start.elapsed().as_millis();
        let finalize_start = Instant::now();
        session.reset_persist_state(path.exists());
        session.reset_provider_messages_cache();
        session.mark_memory_profile_dirty();
        let finalize_ms = finalize_start.elapsed().as_millis();
        crate::logging::info(&format!(
            "[TIMING] remote_startup_load: session={}, snapshot={}ms, journal={}ms, finalize={}ms, snapshot_bytes={}, journal_bytes={}, journal_entries={}, messages={}, total={}ms",
            session.id,
            snapshot_ms,
            journal_ms,
            finalize_ms,
            snapshot_bytes,
            journal_bytes,
            journal_entries,
            session.messages.len(),
            load_start.elapsed().as_millis(),
        ));
        Ok(session)
    }

    pub fn save(&mut self) -> Result<()> {
        self.updated_at = Utc::now();
        let path = session_path(&self.id)?;
        let journal_path = session_journal_path_from_snapshot(&path);
        let start = std::time::Instant::now();
        let snapshot_bytes_before = file_len_or_zero(&path);
        let journal_bytes_before = file_len_or_zero(&journal_path);
        let current_meta = self.journal_meta();
        let metadata_needs_snapshot = self
            .persist_state
            .last_meta
            .as_ref()
            .is_some_and(|prev| metadata_requires_snapshot(prev, &current_meta));
        let vectors_need_snapshot = !self.persist_state.snapshot_exists
            || self.persist_state.messages_mode == PersistVectorMode::Full
            || self.persist_state.env_snapshots_mode == PersistVectorMode::Full
            || self.persist_state.memory_injections_mode == PersistVectorMode::Full
            || self.persist_state.replay_events_mode == PersistVectorMode::Full
            || self.messages.len() < self.persist_state.messages_len
            || self.env_snapshots.len() < self.persist_state.env_snapshots_len
            || self.memory_injections.len() < self.persist_state.memory_injections_len
            || self.replay_events.len() < self.persist_state.replay_events_len;

        let delta_messages = self
            .messages
            .len()
            .saturating_sub(self.persist_state.messages_len);
        let delta_env_snapshots = self
            .env_snapshots
            .len()
            .saturating_sub(self.persist_state.env_snapshots_len);
        let delta_memory_injections = self
            .memory_injections
            .len()
            .saturating_sub(self.persist_state.memory_injections_len);
        let delta_replay_events = self
            .replay_events
            .len()
            .saturating_sub(self.persist_state.replay_events_len);

        let metadata_needs_journal_append = self
            .persist_state
            .last_meta
            .as_ref()
            .is_some_and(|prev| metadata_requires_journal_append(prev, &current_meta));
        if !metadata_needs_snapshot
            && !vectors_need_snapshot
            && !metadata_needs_journal_append
            && delta_messages == 0
            && delta_env_snapshots == 0
            && delta_memory_injections == 0
            && delta_replay_events == 0
        {
            self.reset_persist_state(true);
            return Ok(());
        }

        let (
            result,
            save_mode,
            entry_build_ms,
            append_ms,
            journal_stat_ms,
            checkpoint_ms,
            journal_bytes_after,
        ) = if metadata_needs_snapshot || vectors_need_snapshot {
            let checkpoint_start = Instant::now();
            let result = self.checkpoint_snapshot(&path, &journal_path);
            let checkpoint_ms = checkpoint_start.elapsed().as_millis();
            let journal_bytes_after = file_len_or_zero(&journal_path);
            (
                result,
                "snapshot",
                0,
                0,
                0,
                checkpoint_ms,
                journal_bytes_after,
            )
        } else {
            let entry_build_start = Instant::now();
            let entry = SessionJournalEntry {
                meta: current_meta.clone(),
                append_messages: self.messages[self.persist_state.messages_len..].to_vec(),
                append_env_snapshots: self.env_snapshots[self.persist_state.env_snapshots_len..]
                    .to_vec(),
                append_memory_injections: self.memory_injections
                    [self.persist_state.memory_injections_len..]
                    .to_vec(),
                append_replay_events: self.replay_events[self.persist_state.replay_events_len..]
                    .to_vec(),
            };
            let entry_build_ms = entry_build_start.elapsed().as_millis();
            let append_start = Instant::now();
            let append_result = storage::append_json_line_fast(&journal_path, &entry);
            let append_ms = append_start.elapsed().as_millis();
            match append_result {
                Ok(()) => {
                    self.reset_persist_state(true);
                    let journal_stat_start = Instant::now();
                    let journal_bytes_after = file_len_or_zero(&journal_path);
                    let journal_stat_ms = journal_stat_start.elapsed().as_millis();
                    if journal_bytes_after > MAX_SESSION_JOURNAL_BYTES {
                        let checkpoint_start = Instant::now();
                        let result = self.checkpoint_snapshot(&path, &journal_path);
                        let checkpoint_ms = checkpoint_start.elapsed().as_millis();
                        let journal_bytes_after = file_len_or_zero(&journal_path);
                        (
                            result,
                            "append+checkpoint",
                            entry_build_ms,
                            append_ms,
                            journal_stat_ms,
                            checkpoint_ms,
                            journal_bytes_after,
                        )
                    } else {
                        (
                            Ok(()),
                            "append",
                            entry_build_ms,
                            append_ms,
                            journal_stat_ms,
                            0,
                            journal_bytes_after,
                        )
                    }
                }
                Err(err) => {
                    crate::logging::warn(&format!(
                        "Session journal append failed for {} ({}); checkpointing full snapshot",
                        self.id, err
                    ));
                    let checkpoint_start = Instant::now();
                    let result = self.checkpoint_snapshot(&path, &journal_path);
                    let checkpoint_ms = checkpoint_start.elapsed().as_millis();
                    let journal_bytes_after = file_len_or_zero(&journal_path);
                    (
                        result,
                        "append_failed_fallback_snapshot",
                        entry_build_ms,
                        append_ms,
                        0,
                        checkpoint_ms,
                        journal_bytes_after,
                    )
                }
            }
        };
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 50 {
            crate::logging::info(&format!(
                "Session save slow: total={:.0}ms mode={} metadata_snapshot={} vectors_snapshot={} entry_build={}ms append={}ms journal_stat={}ms checkpoint={}ms messages={} delta_messages={} delta_env_snapshots={} delta_memory_injections={} delta_replay_events={} snapshot_bytes_before={} journal_bytes_before={} journal_bytes_after={}",
                elapsed.as_secs_f64() * 1000.0,
                save_mode,
                metadata_needs_snapshot,
                vectors_need_snapshot,
                entry_build_ms,
                append_ms,
                journal_stat_ms,
                checkpoint_ms,
                self.messages.len(),
                delta_messages,
                delta_env_snapshots,
                delta_memory_injections,
                delta_replay_events,
                snapshot_bytes_before,
                journal_bytes_before,
                journal_bytes_after,
            ));
        }
        result
    }
}
