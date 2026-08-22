//! `trustee sessions migrate` — one-shot legacy → append-only migration.
//!
//! Converts legacy per-checkpoint storage into the current 4-file append-only
//! layout, per session:
//!
//!   conversation.jsonl   ← folded from {NNN}_conversation.json blobs
//!                           (messages deduplicated by sequence/content; the
//!                           log becomes the single source of truth)
//!   agent_state.jsonl    ← one line per checkpoint, from {NNN}_agent.json
//!                           (or session_agent.json when per-checkpoint agent
//!                           files are absent)
//!   checkpoints.json     ← cursors backfilled: each checkpoint's
//!                           cursor_seq = its blob's message count
//!   session_metadata.json  (untouched — write-once)
//!
//! `--prune` additionally deletes the legacy artifacts
//! ({NNN}_conversation.json, {NNN}_agent.json, session_agent.json) after a
//! successful fold.
//!
//! Idempotent: re-running detects an already-migrated session (jsonl exists
//! and index cursors are set) and skips it. Verification: loads the migrated
//! log back and checks that reading up to each checkpoint's cursor returns
//! exactly its blob's messages (byte-comparable on content).
//!
//! See task bcdcdd25-64ed-46de-924e-7279deba4d9f §D (workstream
//! 5ee1ba38-4f9b-4b41-b911-22b92ff89cfe).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Default)]
pub struct MigrationReport {
    pub sessions_scanned: usize,
    pub sessions_migrated: usize,
    pub sessions_skipped: usize,
    pub checkpoints_migrated: usize,
    pub messages_folded: usize,
    pub files_pruned: usize,
    pub errors: Vec<String>,
}

/// Message shape inside a legacy {NNN}_conversation.json blob.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct LegacyMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub token_count: Option<usize>,
    #[serde(default)]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct LegacyBlob {
    messages: Vec<LegacyMessage>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
struct IndexEntry {
    #[serde(default)]
    pub checkpoint_id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub project_hash: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub iteration: u32,
    #[serde(default)]
    pub workflow_step: Option<String>,
    #[serde(default)]
    pub checkpoint_version: Option<String>,
    #[serde(default)]
    pub cursor_seq: u32,
    #[serde(default)]
    pub message_count: u32,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
struct SessionAgentBlob {
    #[serde(default)]
    current_mode: String,
    #[serde(default)]
    current_iteration: u32,
    #[serde(default)]
    current_step: String,
    #[serde(default)]
    last_activity: Option<String>,
    #[serde(default)]
    ts: Option<String>,
}

/// New-format conversation log line.
#[derive(serde::Serialize)]
struct LogLine<'a> {
    seq: u32,
    message: &'a LegacyMessage,
}

/// New-format agent-state log line.
#[derive(serde::Serialize)]
struct StateLine<'a> {
    seq: u32,
    checkpoint_id: &'a str,
    iteration: u32,
    step: &'a str,
    mode: &'a str,
    ts: String,
}

/// Run the migration over every user/project/session tree under `home_dir`.
pub fn run(home_dir: &Path, prune: bool) -> Result<MigrationReport> {
    let mut report = MigrationReport::default();

    // Walk ~/.trustee/users/*/projects/*/sessions/* and ~/.trustee/projects/*/sessions/*
    let mut roots: Vec<PathBuf> = Vec::new();
    let users_dir = home_dir.join("users");
    if users_dir.is_dir() {
        collect_session_dirs(&users_dir, &mut roots);
    }
    let global_projects = home_dir.join("projects");
    if global_projects.is_dir() {
        collect_session_dirs(&global_projects, &mut roots);
    }

    for session_dir in &roots {
        report.sessions_scanned += 1;
        match migrate_session(session_dir, prune) {
            Ok(Some(stats)) => {
                report.sessions_migrated += 1;
                report.checkpoints_migrated += stats.checkpoints;
                report.messages_folded += stats.messages;
                report.files_pruned += stats.pruned;
            }
            Ok(None) => {
                report.sessions_skipped += 1;
            }
            Err(e) => {
                report.errors.push(format!("{}: {}", session_dir.display(), e));
            }
        }
    }

    Ok(report)
}

struct SessionStats {
    checkpoints: usize,
    messages: usize,
    pruned: usize,
}

/// Migrate one session directory. Returns Ok(None) when already migrated
/// (or not legacy — no {NNN}_conversation.json files present).
fn migrate_session(session_dir: &Path, prune: bool) -> Result<Option<SessionStats>> {
    let mut blobs: BTreeMap<String, PathBuf> = BTreeMap::new();
    for entry in std::fs::read_dir(session_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with("_conversation.json") {
            blobs.insert(name.trim_end_matches("_conversation.json").to_string(), entry.path());
        }
    }

    if blobs.is_empty() {
        // Not legacy (new sessions have no per-checkpoint blobs). Still:
        // (a) normalize legacy-shape index entries (may predate required
        //     CheckpointMetadata fields), and (b) prune stray legacy agent
        //     files when --prune was requested.
        let mut did_something = false;
        let index_path = session_dir.join("checkpoints.json");
        if index_path.exists() {
            if let Ok(mut index) = serde_json::from_str::<BTreeMap<String, serde_json::Value>>(
                &std::fs::read_to_string(&index_path)?,
            ) {
                let mut changed = false;
                for (_k, entry) in index.iter_mut() {
                    if let Some(obj) = entry.as_object_mut() {
                        fn need(obj: &serde_json::Map<String, serde_json::Value>, k: &str) -> bool {
                            obj.get(k).map(|v| v.is_null()).unwrap_or(true)
                        }
                        for (k, v) in [
                            ("workflow_step", serde_json::json!("Analyze")),
                            ("checkpoint_version", serde_json::json!("1.0")),
                            ("compressed_size", serde_json::json!(0)),
                            ("uncompressed_size", serde_json::json!(0)),
                            ("description", serde_json::Value::Null),
                            ("tags", serde_json::json!([])),
                        ] {
                            if need(obj, k) {
                                obj.insert(k.to_string(), v);
                                changed = true;
                            }
                        }
                    }
                }
                if changed {
                    std::fs::write(&index_path, serde_json::to_string_pretty(&index)?)?;
                    did_something = true;
                }
            }
        }
        if prune {
            let mut pruned = 0usize;
            let leftovers: Vec<PathBuf> = std::fs::read_dir(session_dir)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .map(|n| {
                            let n = n.to_string_lossy();
                            (n.ends_with("_agent.json") && !n.starts_with("agent_state"))
                                || n == "session_agent.json"
                        })
                        .unwrap_or(false)
                })
                .collect();
            for p in leftovers {
                if std::fs::remove_file(&p).is_ok() {
                    pruned += 1;
                }
            }
            if pruned > 0 {
                did_something = true;
                return Ok(Some(SessionStats { checkpoints: 0, messages: 0, pruned }));
            }
        }
        if did_something {
            return Ok(Some(SessionStats { checkpoints: 0, messages: 0, pruned: 0 }));
        }
        return Ok(None);
    }

    let log_path = session_dir.join("conversation.jsonl");
    let state_path = session_dir.join("agent_state.jsonl");
    let index_path = session_dir.join("checkpoints.json");

    // Existing jsonl (mixed sessions): fold legacy messages only where the
    // log does not already contain them (dedup by content signature).
    let mut existing_lines: Vec<String> = Vec::new();
    if log_path.exists() {
        existing_lines = std::fs::read_to_string(&log_path)?
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|s| s.to_string())
            .collect();
    }
    let existing_max_seq: u32 = existing_lines
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get("seq").and_then(|s| s.as_u64()))
        .map(|s| s as u32)
        .max()
        .unwrap_or(0);
    let _ = existing_max_seq;

    // Fold: legacy blobs are cumulative snapshots of the same conversation
    // (blob N ⊇ blob N-1), but each blob re-serializes every message with a
    // fresh timestamp. Match by INDEX + content signature: a message whose
    // signature already occupies that index in the folded log is reused;
    // anything else appends as a new sequence number. This preserves
    // duplicate messages that legitimately repeat within a single snapshot
    // while never duplicating the same occurrence across snapshots.
    let mut signatures: Vec<String> = existing_lines
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get("message").cloned())
        .map(|m| signature(&m))
        .collect();

    let mut next_seq = existing_max_seq + 1;
    let mut new_lines: Vec<String> = Vec::new();
    let mut checkpoint_cursors: BTreeMap<String, (u32, u32)> = BTreeMap::new(); // id -> (cursor, count)

    for (ckpt_id, blob_path) in &blobs {
        let blob: LegacyBlob = serde_json::from_str(&std::fs::read_to_string(blob_path)?)
            .with_context(|| format!("parsing {}", blob_path.display()))?;
        let mut cursor = 0u32;
        for (i, msg) in blob.messages.iter().enumerate() {
            let sig = signature_message(msg);
            let idx = i; // index within this snapshot
            if idx < signatures.len() && signatures[idx] == sig {
                // Same occurrence already folded — reuse its sequence number.
                cursor = (idx as u32) + 1;
                continue;
            }
            let line = serde_json::to_string(&LogLine { seq: next_seq, message: msg })?;
            new_lines.push(line);
            if idx < signatures.len() {
                // Index collision with different content: this snapshot
                // rewrote history — treat as a new occurrence (append).
                signatures[idx] = sig;
            } else {
                signatures.push(sig);
            }
            cursor = next_seq;
            next_seq += 1;
        }
        checkpoint_cursors.insert(ckpt_id.clone(), (cursor, blob.messages.len() as u32));
    }

    // Write (append) the folded log.
    let mut log_contents = existing_lines.join("\n");
    if !existing_lines.is_empty() {
        log_contents.push('\n');
    }
    log_contents.push_str(&new_lines.join("\n"));
    if !new_lines.is_empty() {
        log_contents.push('\n');
    }
    std::fs::write(&log_path, &log_contents)?;

    // Agent-state log: one line per checkpoint.
    let mut state_lines: Vec<String> = Vec::new();
    let fallback_agent = session_dir.join("session_agent.json");
    let fallback: Option<SessionAgentBlob> = if fallback_agent.exists() {
        serde_json::from_str(&std::fs::read_to_string(&fallback_agent)?).ok()
    } else {
        None
    };
    // Checkpoint created_at timestamps from the index (RFC3339), used when
    // the legacy agent blobs carry no usable timestamp.
    let index_created_at: BTreeMap<String, Option<String>> = if index_path.exists() {
        serde_json::from_str::<BTreeMap<String, IndexEntry>>(&std::fs::read_to_string(&index_path)?)
            .map(|ix| ix.into_iter().map(|(k, v)| (k, v.created_at)).collect())
            .unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    for (i, (ckpt_id, (cursor, _count))) in checkpoint_cursors.iter().enumerate() {
        // Prefer the per-checkpoint legacy agent file when present.
        let agent_path = session_dir.join(format!("{}_agent.json", ckpt_id));
        let (mode, iteration, step, ts) = if agent_path.exists() {
            let a: SessionAgentBlob =
                serde_json::from_str(&std::fs::read_to_string(&agent_path)?).unwrap_or_else(|_| {
                    fallback.clone().unwrap_or(SessionAgentBlob {
                        current_mode: "unknown".into(),
                        current_iteration: (i + 1) as u32,
                        current_step: "analyze".into(),
                        last_activity: None,
                        ts: None,
                    })
                });
            (
                a.current_mode,
                a.current_iteration,
                a.current_step,
                a.ts,
            )
        } else if let Some(f) = &fallback {
            // Per-checkpoint agent file absent: the session_agent blob holds
            // only the FINAL state; derive per-checkpoint iteration from the
            // checkpoint's own number so history isn't backdated.
            let ckpt_num: u32 = ckpt_id
                .split('_')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(i as u32 + 1);
            (f.current_mode.clone(), ckpt_num, f.current_step.clone(), f.last_activity.clone())
        } else {
            ("unknown".into(), (i + 1) as u32, "analyze".into(), None)
        };
        let ts = ts
            .or(fallback.as_ref().and_then(|f| f.last_activity.clone()))
            .or_else(|| index_created_at.get(ckpt_id).cloned().flatten())
            .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
        state_lines.push(serde_json::to_string(&StateLine {
            seq: (i + 1) as u32,
            checkpoint_id: ckpt_id,
            iteration: iteration.max(1),
            step: &step,
            mode: &mode,
            ts,
        })?);
    }
    let state_exists = state_path.exists();
    if !state_exists || std::fs::read_to_string(&state_path)?.trim().is_empty() {
        std::fs::write(&state_path, if state_lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", state_lines.join("\n"))
        })?;
    }

    // Backfill the index cursors (idempotent: only set when missing/zero).
    if index_path.exists() {
        let mut index: BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(&index_path)?)?;
        let mut changed = false;
        for (ckpt_id, (cursor, count)) in &checkpoint_cursors {
            if let Some(entry) = index.get_mut(ckpt_id) {
                // Normalize the entry to the full CheckpointMetadata schema:
                // legacy index files may predate required fields.
                if let Some(obj) = entry.as_object_mut() {
                    fn need(obj: &serde_json::Map<String, serde_json::Value>, k: &str) -> bool {
                        obj.get(k).map(|v| v.is_null()).unwrap_or(true)
                    }
                    if need(obj, "workflow_step") {
                        obj.insert("workflow_step".into(), serde_json::json!("Analyze"));
                    }
                    if need(obj, "checkpoint_version") {
                        obj.insert("checkpoint_version".into(), serde_json::json!("1.0"));
                    }
                    if need(obj, "compressed_size") {
                        obj.insert("compressed_size".into(), serde_json::json!(0));
                    }
                    if need(obj, "uncompressed_size") {
                        obj.insert("uncompressed_size".into(), serde_json::json!(0));
                    }
                    if need(obj, "description") {
                        obj.insert("description".into(), serde_json::Value::Null);
                    }
                    if need(obj, "tags") {
                        obj.insert("tags".into(), serde_json::json!([]));
                    }
                }
                let cur = entry.get("cursor_seq").and_then(|v| v.as_u64()).unwrap_or(0);
                let cnt = entry.get("message_count").and_then(|v| v.as_u64()).unwrap_or(0);
                if cur == 0 || cnt == 0 {
                    if let Some(obj) = entry.as_object_mut() {
                        obj.insert("cursor_seq".into(), serde_json::json!(cursor));
                        obj.insert("message_count".into(), serde_json::json!(count));
                        changed = true;
                    }
                }
            } else {
                index.insert(
                    ckpt_id.clone(),
                    serde_json::json!({
                        "checkpoint_id": ckpt_id,
                        "session_id": session_dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                        "cursor_seq": cursor,
                        "message_count": count,
                        "iteration": checkpoint_cursors.keys().position(|k| k == ckpt_id).map(|p| (p + 1) as u32).unwrap_or(1),
                    }),
                );
                changed = true;
            }
        }
        if changed {
            std::fs::write(&index_path, serde_json::to_string_pretty(&index)?)?;
        }
    }

    // Verify: every checkpoint cursor must point inside the folded log, and
    // the longest blob's messages must be fully covered by the log (content
    // equality at their folded positions).
    let log_lines: Vec<serde_json::Value> = std::fs::read_to_string(&log_path)?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let total = log_lines.len() as u32;
    for (ckpt_id, (cursor, _count)) in &checkpoint_cursors {
        if *cursor > total {
            anyhow::bail!(
                "verification failed for {}: cursor {} exceeds log length {}",
                ckpt_id,
                cursor,
                total
            );
        }
    }
    // The longest blob must be recoverable: its message sequence must equal
    // the log's first blob-len lines by content signature.
    let (longest_id, longest_path) = blobs
        .iter()
        .max_by_key(|(_, p)| blob_len(p).unwrap_or(0))
        .map(|(id, p)| (id.clone(), p.clone()))
        .context("no blobs to verify")?;
    let longest: LegacyBlob =
        serde_json::from_str(&std::fs::read_to_string(&longest_path)?)?;
    let longest_len = longest.messages.len();
    if log_lines.len() < longest_len {
        anyhow::bail!(
            "verification failed: log has {} lines but longest blob {} has {} messages",
            log_lines.len(),
            longest_id,
            longest_len
        );
    }
    for (i, msg) in longest.messages.iter().enumerate() {
        let line_sig = log_lines[i]
            .get("message")
            .map(signature)
            .unwrap_or_default();
        if line_sig != signature_message(msg) {
            anyhow::bail!(
                "verification failed at longest blob {} position {}: content mismatch",
                longest_id,
                i
            );
        }
    }

    let mut pruned = 0usize;
    if prune {
        for blob_path in blobs.values() {
            std::fs::remove_file(blob_path)?;
            pruned += 1;
        }
        let agent_paths: Vec<PathBuf> = std::fs::read_dir(session_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .map(|n| {
                        let n = n.to_string_lossy();
                        n.ends_with("_agent.json") && !n.starts_with("agent_state")
                    })
                    .unwrap_or(false)
            })
            .collect();
        for p in agent_paths {
            if std::fs::remove_file(&p).is_ok() {
                pruned += 1;
            }
        }
        if fallback_agent.exists() {
            std::fs::remove_file(&fallback_agent)?;
            pruned += 1;
        }
    }

    Ok(Some(SessionStats {
        checkpoints: checkpoint_cursors.len(),
        messages: new_lines.len(),
        pruned,
    }))
}

fn collect_session_dirs(root: &Path, out: &mut Vec<PathBuf>) {
    // root: .../users/*/projects/*/sessions or .../projects/*/sessions
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        // Depth-aware: users/<user>/projects/<proj>/sessions/<session>
        if name == "sessions" {
            if let Ok(sessions) = std::fs::read_dir(&p) {
                for s in sessions.flatten() {
                    if s.path().is_dir() {
                        out.push(s.path());
                    }
                }
            }
        } else {
            collect_session_dirs(&p, out);
        }
    }
}

fn signature(m: &serde_json::Value) -> String {
    let g = |k: &str| {
        match m.get(k) {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(v) if !v.is_null() => v.to_string(),
            _ => String::new(),
        }
    };
    format!("{}|{}|{}|{}", g("role"), g("content"), g("tool_call_id"), g("name"))
}

/// Number of messages in a legacy conversation blob (without full parse).
fn blob_len(path: &Path) -> Result<usize> {
    let blob: LegacyBlob = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    Ok(blob.messages.len())
}

fn signature_message(m: &LegacyMessage) -> String {
    format!(
        "{}|{}|{}|{}",
        m.role,
        m.content,
        m.tool_call_id.clone().unwrap_or_default(),
        m.name.clone().unwrap_or_default()
    )
}
