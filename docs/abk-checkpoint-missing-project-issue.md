## ABK Checkpoint: Resume fails when project_path in metadata.json points to a deleted folder

Summary
-------
When using `trustee` (which relies on `abk`'s checkpoint subsystem) the `resume` flow fails with an error when a project's `metadata.json` contains a `project_path` that no longer exists on disk. Instead of skipping that project entry, the code attempts to canonicalize the missing path and returns a storage error that aborts the resume operation.

Observed behavior
-----------------
- Running `trustee resume` (or the ABK CLI resume path used by `trustee`) when the checkpoint storage contains a project metadata file whose `project_path` points at a deleted directory results in an error similar to:

```
Error: CheckpointError("Failed to get project storage: Storage error: Failed to canonicalize project path /data/Projects/podtan/tangent-grok: No such file or directory (os error 2)")
```

- The resume operation aborts instead of ignoring that stale project entry.

Where this happens (call chain)
-------------------------------
- User triggers `trustee resume` → ABK CLI resume code (`abk/src/cli/commands/resume.rs`) invokes the checkpoint adapter.
- The adapter (`abk/src/cli/runner.rs`, `AbkCheckpointAccess`) calls `crate::checkpoint::get_storage_manager()` and then `manager.get_project_storage(project_path)`.
- `CheckpointStorageManager::get_project_storage` (in `abk/src/checkpoint/storage.rs`) internally calls `ProjectHash::new(project_path)`.
- `ProjectHash::new` (in `abk/src/checkpoint/models.rs`) canonicalizes the provided `project_path` using `project_path.canonicalize()`; this fails when the path does not exist and returns a `CheckpointError::storage` error, which bubbles up and becomes the error reported to the user.

Relevant code locations
-----------------------
- `abk/src/cli/commands/resume.rs` — entry point for the CLI resume flow and where `list_projects`/`list_sessions` are used.
- `abk/src/cli/runner.rs` — `AbkCheckpointAccess` implementation that calls into the checkpoint manager (`get_storage_manager()` and `get_project_storage`).
- `abk/src/checkpoint/storage.rs` — `CheckpointStorageManager::get_project_storage` and `load_or_create_project_metadata` (metadata handling).
- `abk/src/checkpoint/models.rs` — `ProjectHash::new` where `project_path.canonicalize()` is performed.

Reproduction steps (minimal)
---------------------------
1. Create a metadata file for a project under the agent storage, for example:

   - Path: `~/.{agent_name}/projects/<project-hash>/metadata.json`
   - Content (example):

```json
{
  "project_hash": "<hash>",
  "project_path": "/data/Projects/podtan/tangent-grok",
  "name": "tangent-grok",
  "created_at": "2025-01-01T00:00:00Z",
  "last_accessed": "2025-01-01T00:00:00Z",
  "session_count": 1,
  "size_bytes": 12345,
  "git_remote": null
}
```

2. Ensure `/data/Projects/podtan/tangent-grok` does not exist (delete or rename it).
3. Run `trustee resume` (or the ABK resume flow used by the `trustee` binary).
4. Observe the canonicalize/storage error and aborted resume.

Notes and expectations (not a fix)
---------------------------------
- The code currently expects a project path listed in stored metadata to be canonicalizable at the time it is used (for hashing and storage lookup). When the path is stale (deleted/renamed), canonicalization fails and the manager returns an error which aborts higher-level CLI commands.
- A reasonable expectation for users is that resume/listing operations should tolerate stale or orphaned metadata entries (for example, skip entries where the canonicalize or project access fails) rather than failing the entire operation.

What I captured
----------------
- Exact call chain and where canonicalization occurs.
- Example error message observed when a missing `project_path` is encountered.
- Minimal reproduction steps and JSON example for the `metadata.json` that triggers the issue.

References
----------
- `abk/src/checkpoint/models.rs` — `ProjectHash::new` (canonicalize usage).
- `abk/src/checkpoint/storage.rs` — `CheckpointStorageManager::get_project_storage` and `load_or_create_project_metadata` (project metadata loading and creation).
- `abk/src/cli/runner.rs` — `AbkCheckpointAccess` adapter that calls into the checkpoint manager.
- `abk/src/cli/commands/resume.rs` — CLI resume flow where the error surface is observed.

This document intentionally describes the issue and reproduction steps only and does not propose or implement a fix.
