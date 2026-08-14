# Changelog

All notable changes to this project will be documented in this file.

## [0.9.11] - 2026-08-10

### Added
- **feat(web): model picker UI + unified app menu** — The burger menu is now a unified dropdown containing Model, Sessions, and MCP Connections (replacing the separate header MCP button). Model selection lists `[llm.provider]` (default) and all `[llm.providers.*]` entries from the user's merged config; the choice is persisted in `localStorage` and sent as `model` on every command.
  - **API**: New `GET /api/v1/models` endpoint returning `{ default: {model}, models: [{id, model}] }`. Resolves the per-user config (shared + overlay + `${VAR}` substitution) via the new `ServerState::resolve_user_config()` — extracted from `apply_user_isolation` into reusable `load_user_secrets()` + `merge_user_config()`. Secrets (api_key, base_url) are never exposed.
  - **Web UI**: Model picker overlay, unified burger menu with current-model value shown inline, selection persisted per browser.

### Changed
- **deps: bump trustee-api to 0.7.19, trustee-web to 0.1.14**

## [0.9.10] - 2026-08-10

### Added
- **feat: multi-provider LLM selection** — Define multiple named LLM providers in TOML via `[llm.providers.{name}]`, then select which one to use per-command via the `model` field in API requests. The selected provider's config (model, base_url, api_key) overrides `[llm.provider]` for that command only. Works with per-user config isolation — each user can define their own providers.
  - **ABK** (0.12.16): Added `providers: HashMap<String, ProviderConfig>` to `LlmConfig`.
  - **trustee-core** (0.6.10): Added `Session.model: Option<String>` field. `inject_model()` helper replaces `[llm.provider]` with the selected `[llm.providers.{name}]` entry in the config TOML clone before each command.
  - **trustee-api** (0.7.18): Added `model: Option<String>` to `CommandRequest`, `CreateSessionRequest`, and `NewSessionRequest`.
  - **Config**: Added commented `[llm.providers.*]` examples to `trustee_default.toml`.

### Changed
- **deps: bump abk to 0.12.16, trustee-core to 0.6.10, trustee-api to 0.7.18, trustee-tui to 0.3.5**

## [0.9.9] - 2026-08-10

### Added
- **feat: TOML-based LLM provider configuration** — LLM model, base_url, and api_key can now be configured in TOML via `[llm.provider]`, using the same `${ENV_VAR}` substitution pattern as MCP credentials. This enables per-user LLM config in multi-tenant web mode (each user's `~/.trustee/users/{hash}/config/trustee.toml` can specify a different model/base_url). Environment variables (`OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_DEFAULT_MODEL`) remain as fallback when `[llm.provider]` is absent — fully backward-compatible.
  - **ABK** (0.12.14): New `ProviderConfig` struct, `OpenAIProvider::with_config()`, `ProviderFactory::create_with_config()`, threaded from `Agent::new_from_config()` and `runner.rs` callers.
  - **Default config** (`trustee_default.toml`): Added `[llm.provider]` section with `name`, `model`, `base_url`, `api_key = "${OPENAI_API_KEY}"`.

### Changed
- **deps: bump abk to 0.12.14, trustee-core to 0.6.9, trustee-api to 0.7.17, trustee-tui to 0.3.4**

## [0.9.8] - 2026-08-10

### Added
- **feat: optional agent identity injection** — Trustee now accepts an optional identity string that is prepended to `lifecycle.system_template` at runtime, enabling agents to load a self-model (e.g. from Fame) before the agent loop starts. Fully backward-compatible: if no identity is provided, the default system template is used unchanged.
  - **Session layer** (`trustee-core`): `Session.identity: Option<String>` field, injected into the config clone inside `execute_command()` and `trigger_handoff()` — never written to disk, fresh per session.
  - **TUI**: reads `TRUSTEE_IDENTITY` env var at startup, passes through `trustee_tui::run()`.
  - **Web/API**: `identity` field added to `CommandRequest`, `CreateSessionRequest`, and `NewSessionRequest` (all `#[serde(default)]`). Each session can carry a different identity.
  - **CLI**: reads `TRUSTEE_IDENTITY` env var, injects directly into config string before `run_from_raw_config()`.

### Changed
- **deps: bump trustee-core to 0.6.8, trustee-api to 0.7.16, trustee-tui to 0.3.3**

## [0.9.5] - 2026-08-09

### Fixed
- **fix(web): handoff status line now appears live (no refresh needed)** — `trigger_handoff()` was pushing `"🔀 Generating session handoff briefing..."` directly to `output_lines`, which is only served via the `/api/v1/session` poll. It now sends the line through the `TuiMessage` channel as `OutputLine`, so the WebSocket drain task pushes it to `output_lines` AND broadcasts it live (frontend `appendOutput` + `StateChanged Running`). Same fix for `"⏹ Cancelling before handoff..."` in `request_handoff()`. The user now sees immediate feedback when clicking Handoff.

### Changed
- **deps: bump trustee-core to 0.6.7, trustee-api to 0.7.14**

## [0.9.4] - 2026-08-09

### Fixed
- **fix(web): session-scoped routes resolve by checkpoint/session id OR live MSU key** — `POST /api/v1/sessions/{id}/handoff` (and command/cancel/name/stream/live/destroy) now resolve the target session via new `ServerState::get_session_by_any_id()`: first by live MSU registry key (Torpi/THQ), then by `session.session_id` (the checkpoint/continuity id the embedded web UI tracks from `ResumeInfo`). This fixes web handoff returning 404 — the frontend holds `session_YYYY_...` while the live registry is keyed `"default"`/`new_sid`. Reverting to the legacy route is NOT done, preserving multi-session correctness for Torpi/THQ.
- **fix(web): `post_command_session` sets active session using the resolved live key** — avoids setting active to a checkpoint id that isn't a registry key.

### Changed
- **deps: bump trustee-api to 0.7.13**

## [0.9.3] - 2026-08-09

### Fixed
- **fix(handoff): replace full-workflow briefing with single direct LLM call** — The handoff briefing previously ran the entire agentic workflow (`run_task_from_raw_config`), which handed the model tools, looped on large checkpoint history (161K+ tokens), ran for 20+ minutes, and never completed — bricking the session in a permanent Running state. The briefing is now generated by `abk::cli::generate_handoff_briefing()`: loads conversation history from the last checkpoint, then ONE `provider.generate()` call with the MAIN model. No tools, no workflow loop, no checkpointing, no session bricking.
- **fix(handoff): remove checkpointing-override + HandoffCaptureSink machinery** — The config-override approach (0.9.2) for disabling checkpointing during briefing is removed; it was treating one symptom of the wrong-tool-for-the-job problem. The direct call writes no checkpoints by construction.

### Changed
- **deps: bump abk to 0.12.12** (new `generate_handoff_briefing`), trustee-core to 0.6.6, trustee-api to 0.7.12

## [0.9.2] - 2026-08-09

### Fixed
- **fix(handoff): safe `request_handoff()` state-machine entry** — Handoff requests while a workflow is running now cancel + queue (mirroring TUI Ctrl+H) instead of spawning a concurrent briefing workflow that corrupts the session (Bug 1/2/5).
- **fix(handoff): preserve `resume_info` on failed/cancelled briefing** — The briefing run now keeps a backup and restores session continuity via `HandoffFailed` if the LLM returns nothing useful (Bug 4/8).
- **fix(handoff): disable checkpointing during briefing** — The briefing no longer writes checkpoints into the old session's chain, keeping history intact (Bug 6).
- **fix(handoff): never auto-execute a garbage briefing** — Empty/truncated briefings surface an error and leave the session preserved instead of being run as a destructive task (Bug 8).
- **fix(web): handoff briefing visible before new task runs** — `HandoffReady` now carries the briefing text and the frontend renders it as an agent bubble before the follow-up task executes (Bug 7).
- **fix(web): handoff button gated on Idle + actual checkpoint availability** — Button is disabled while running/cancelling and until a checkpoint (`resume_info`) exists; no more silent no-op (Bug 2/3).
- **fix(web): handoff uses session-scoped route** — `/api/v1/sessions/{id}/handoff` targets the correct session in multi-session mode (Bug 9).

### Changed
- **deps: bump trustee-core to 0.6.5, trustee-web to 0.1.13, trustee-api to 0.7.11**

## [0.9.1] - 2026-08-09

### Fixed
- **fix(config): default `[llm.utility] max_tokens` raised from 100 to 1000** — Thinking models like GLM-4.7-Flash need 500-600+ reasoning tokens before producing a title. The old default caused truncated responses with empty content, resulting in no LLM title being generated.
- **deps: bump abk to 0.12.10** — Title generation now retries with higher max_tokens on truncation, improved reasoning extraction for GLM "Idea N:" brainstorming patterns.

### Changed
- **deps: bump trustee-core to 0.6.4, trustee-api to 0.7.10**

## [0.9.0] - 2026-08-09

### Fixed
- **fix(session): description survives session resume** — `create_session_with_description` now reads existing metadata instead of overwriting with null. Preserves description/tags on resume.
- **fix(session): web sessions get description at creation** — Falls back to task description when `SessionIdentity.name` is None.

### Changed
- **deps: bump abk to 0.12.9, trustee-core to 0.6.3, trustee-api to 0.7.9**

## [0.8.8] - 2026-08-08

### Fixed
- **fix(session): skip title generation for resumed sessions** — Resumed sessions (continuations) were getting their titles overwritten with garbage from thinking model reasoning. Now only fresh sessions (first command, no `resume_info`) trigger title generation.
- **fix(session): race condition mitigation** — Added 500ms delay before title persist to ensure checkpoint metadata writes complete first, preventing the checkpoint system from overwriting the LLM title.

### Changed
- **deps: bump trustee-core to 0.6.1, trustee-api to 0.7.7**

## [0.8.7] - 2026-08-08

### Fixed
- **fix(session): title generation no longer overwrites existing LLM titles** — Added `should_generate_title()` guard that checks whether the session description is still the raw truncated command (needs LLM title) or has already been set. Prevents title churn when multiple commands run in the same session.

### Changed
- **deps: bump abk to 0.12.7** — `should_generate_title()` + improved reasoning extraction.
- **deps: bump trustee-core to 0.6.0, trustee-api to 0.7.6**

## [0.8.6] - 2026-08-08

### Fixed
- **fix(session): LLM titles now persisted to remote backend** — `persist_session_title()` now accepts `config_toml` and writes to DocumentDB/MongoDB when configured, in addition to local filesystem.

### Changed
- **deps: bump abk to 0.12.6** — Remote backend support in persist_session_title.
- **deps: bump trustee-core to 0.5.9, trustee-api to 0.7.5**

## [0.8.5] - 2026-08-08

### Fixed
- **fix(session): LLM session titles now work for CLI path** — Title generation was only in trustee-core's `execute_command()` (web/TUI). The CLI `run` command bypasses that entirely. Added title generation + persistence to main.rs after `run_from_raw_config` succeeds.
- **fix(session): LLM titles persisted to disk** — `persist_session_title()` writes directly to `session_metadata.json` via atomic file operations, independent of the ABK SessionManager lifecycle.

### Changed
- **deps: bump abk to 0.12.5** — Thinking model support + persist_session_title.
- **deps: bump trustee-core to 0.5.8, trustee-api to 0.7.4**

## [0.8.4] - 2026-08-08

### Fixed
- **fix(session): persist LLM-generated titles to disk** — LLM-generated session titles were only updated in memory (via `SessionTitleUpdated` message) but never written to `session_metadata.json`. Now after generating a title, `persist_session_title()` writes it directly to disk via atomic file operations. The on-disk `description` field now reflects the LLM-generated title instead of the raw truncated command text.

### Changed
- **deps: bump abk to 0.12.4** — Adds `persist_session_title()` standalone function.
- **deps: bump trustee-core to 0.5.7, trustee-api to 0.7.3**

## [0.8.3] - 2026-08-03

### Added
- **feat(session): LLM-generated session titles** — After a workflow completes successfully, a lightweight LLM call generates a concise (≤50 chars) descriptive session title, replacing the truncated-command placeholder. Fire-and-forget: errors don't affect the session. Configurable via optional `[llm.utility]` section (model, max_tokens, temperature).
- **feat(core): `SessionTitleUpdated` TuiMessage variant** — New message type for asynchronous session title updates from background LLM calls. Handled in `handle_workflow_message()` to update `session_name`.
- **feat(config): `[llm.utility]` documentation** — Added commented-out example config for utility LLM in `trustee_default.toml`.

### Fixed
- **fix(checkpoint): persist session description after first checkpoint** — Session titles (descriptions) were only written at creation and never updated in `session_metadata.json`. Now persisted after the first checkpoint via abk's new `update_session_description()`.

### Changed
- **deps: bump abk to 0.12.3** — Brings `[llm.utility]` config, `generate_session_title()`, and `update_session_description()` API.
- **deps: bump trustee-core to 0.5.6, trustee-api to 0.7.2**

## [0.2.7] - 2026-07-25

### Added
- **feat(thq): auto-register trustee-web with Torpi on startup** — New `[thq]` config section. When present, trustee-web spawns a background task that POSTs to Torpi's `/thq/api/agents` endpoint on startup and re-registers every `heartbeat_interval` seconds (default 30s). Agent identity is a stable UUID v4 persisted to `~/.trustee/agent_id`. Config: `torpi_url`, `advertise_url`, `agent_name`, `agent_role`, `capabilities`, `tags`, `heartbeat_interval`.
- **feat(web): redesign header — burger menu, centered title, no icon** — Header now uses a burger menu for session browsing, centered title, and groups user avatar + context token badge in the right section.

### Fixed
- **fix(thq): correct registration endpoint to POST /thq/api/agents** — Initial implementation used `/thq/api/agents/register` which does not exist; corrected to match Torpi's router definition.
- **fix(api): pin pep to 0.4.3 for session_manager module availability** — Ensures `WebSessionManager` and related modules are available from PEP.
- **fix(web): group tokens + user in header-right div** — Header layout fix to prevent overlapping elements.
- **fix(auth): reject stale dev tokens when dev mode is disabled** — Dev-mode tokens (`dev:...`) in cookies or Bearer headers are now rejected when `local_dev_mode = false`, preventing stale dev sessions from bypassing OIDC.
- **fix(deps): upgrade abk to 0.8.3** — Cross-project session resume and cancel-checkpoint metadata sync fixes.
- **fix(deps): upgrade abk to 0.8.2 / 0.8.1** — Cancel-checkpoint and metadata sync fixes.
- **fix(api): add force-refresh retry on JWT ExpiredSignature** — When session token validation fails due to expiry, force-refreshes via `WebSessionManager` and retries before returning 401.

### Changed
- **chore: remove sample mcp servers from default config** — Cleaned up sample MCP server entries from config files.
- **chore: use abk 0.8.3 from crates.io** — Removed local path overrides.
- **deps: bump trustee-api to 0.1.8.**

## [0.2.6] - 2026-07-23

### Added
- **feat(web): add session browsing and resume from Web UI** — Burger menu opens a sessions overlay listing all sessions with checkpoints across all projects. Each card shows session ID, project, time ago, checkpoint count, and a Resume button.
- **feat(web): load conversation history on session resume** — After resuming, the full conversation history (user messages, agent responses with markdown, tool calls, reasoning) is rendered in the output panel.
- **feat(api): integrate PEP 0.4.2 WebSessionManager for session-based auth** — Replaced direct JWT-in-cookie with server-side session management. Browser cookie holds a session_id; access token is stored server-side with auto-refresh.
- **feat(api): upgrade to pep 0.4.3 with idle-timeout eviction and rolling cookies** — Sessions auto-expire after idle timeout; cookies are rolled on every successful auth (sliding window).

### Fixed
- **fix: skip projects with unresolvable paths in session discovery** — Projects with paths that no longer exist are silently skipped instead of causing an error.
- **fix(web): cross-project session resume** — Session resume now works across different project directories.

### Changed
- **chore: bump trustee-web to 0.1.6 (session browser UI).**

## [0.2.5] - 2026-07-22

### Added
- **feat(api): integrate PEP WebSessionManager for session-based auth with auto-refresh** — Tokens are managed server-side; cookies contain session_id instead of raw JWT.
- **feat(api): upgrade to pep 0.4.3 with idle-timeout eviction and rolling cookies** — Idle timeout evicts stale sessions; rolling cookies keep active users logged in.

### Fixed
- **fix(api): add force-refresh retry on JWT ExpiredSignature** — Force-refreshes token and retries once when JWT validation fails due to expiry.

## [0.2.4] - 2026-07-21

### Added
- **feat(api): HTTPS by default with self-signed cert auto-generation** — Trustee-web now serves HTTPS by default using a self-signed certificate from `~/.trustee/certs/`. Use `--no-tls` for plain HTTP.

### Fixed
- **fix(api): install ring crypto provider before TLS init** — Prevents panic when rustls is built without default features.
- **fix(api): WebSocket upgrade support in TLS mode** — Manual TLS accept loop with `hyper-util` auto builder and `serve_connection_with_upgrades` for WS support.
- **fix(web): mobile black box + output under input bar** — Fixes layout issues on mobile where content appeared under the fixed input bar.
- **fix(web): WebSocket reconnect exponential backoff** — WS reconnection now uses exponential backoff with 30s cap.
- **fix(api): lower TLS accept failure log from WARN to DEBUG** — Reduces log noise from expected TLS handshake failures.
- **fix(web): handoff button enabled on page reload with existing session** — Button state correctly reflects session content on reload.

## [0.2.3] - 2026-07-20

### Added
- **feat(web): torpi-style user avatar + dropdown menu in header** — Circular avatar with initials, dropdown menu with name/email and sign-out.

### Fixed
- **fix(web): text selection vanishes — McpServerStatus was re-rendering all output** — MCP status updates no longer cause full output re-render, preserving text selection.
- **fix(auth): PKCE/auth cookies use Secure flag based on redirect_uri scheme** — HTTP localhost/LAN connections no longer set Secure, preventing browser from dropping the cookie.
- **fix(auth): user name/email from userinfo + login screen + sign out** — Fetches name/email from OIDC userinfo endpoint when missing from JWT. Login overlay with "Sign in with SSO" button.
- **fix(web): user avatar not showing — fallback name + margin-left:auto** — Avatar now falls back to name/email/sub.

## [0.2.0] - 2026-07-19

### Added
- **feat(core): extract trustee-core crate (v0.1.0)** — New workspace crate containing shared types (`TuiMessage`, `FocusPanel`, `WorkflowState`, `McpServerStatus`, `McpServerInfo`, `BuildInfo`, `AutoHandoffConfig`, `CapturedText`, `HandoffCaptureSink`), session state (`Session` struct with `handle_workflow_message`, `execute_command`, `trigger_handoff`), config parsing (`parse_auto_handoff_config`), and `TuiForwardSink` (ABK output event → message channel with 3-state stream state machine). `Session::new()` returns `(Session, Receiver)` to prevent deadlock when the receiver is used in async loops.
- **feat(api): add trustee-api crate (v0.1.0)** — New axum 0.8 REST + WebSocket server wrapping `trustee-core::Session`. Endpoints: `GET /api/v1/health`, `GET /api/v1/session`, `POST /api/v1/session/command`, `POST /api/v1/session/cancel`, `POST /api/v1/session/handoff`, `WS /api/v1/session/stream`. Background drain task owns the workflow receiver directly (no mutex deadlock). All deps use rustls (no openssl/native-tls) for static Pi/ARM builds.
- **feat(web): add trustee-web crate (v0.1.0)** — Static web frontend embedded via rust-embed 8.12. Dark-themed UI with output panel, todo sidebar, MCP status, input bar, WebSocket live streaming, cancel/handoff buttons, and context token counter.
- **feat(web): `trustee web` subcommand** — New `web` feature flag starts the API + web server. Usage: `trustee web [--addr 0.0.0.0:3000]`.

### Changed
- **refactor(tui): App wraps Session** — `trustee-tui::App` now contains a `trustee_core::session::Session` field. All workflow logic (`handle_workflow_message`, `execute_command`, `trigger_handoff`) moved to `Session`. TUI modules access session fields via `self.session.*`. Removed `tui_sink.rs` (replaced by `TuiForwardSink` in trustee-core). Removed direct `abk`, `tokio-util`, `unicode-segmentation` deps from trustee-tui.
- **deps: bump trustee-tui to 0.1.55.**

## [0.1.99] - 2026-07-17

### Fixed
- **fix(tui): orphan characters during streaming scroll — manual line slicing** — Replaced `Paragraph` + `.wrap(Wrap { trim: false })` + `.scroll()` in the output and todo panel rendering with manual pre-wrapping and line slicing. The `wrap_line()` helper word-wr each line to viewport width using `unicode-width`, `build_visual_lines()` flattens all output into a single `Vec<Line<'static>>`, and `slice_visible()` returns only the visible window. The `Paragraph` widget now renders with neither `.wrap()` nor `.scroll()`, eliminating the scroll-offset desynchronization that caused orphan/stale characters during LLM streaming (issue #74a21aa5, [ratatui#2342](https://github.com/ratatui-org/ratatui/issues/2342)). Applied to output panel, todo panel, and zoomed modes.

### Changed
- **refactor(tui): split app.rs into 6 modules** — The 1826-line `app.rs` has been split into `types.rs` (type definitions), `helpers.rs` (text wrapping, color parsing, utilities), `render.rs` (rendering logic), `event.rs` (keyboard/mouse/paste handling), `workflow.rs` (workflow message processing, command execution, clipboard), and a slimmed `app.rs` (struct, constructor, main loop, config parsing). No functional changes.
- **deps: bump trustee-tui to 0.1.54.**

## [0.1.98] - 2026-07-17

### Added
- **feat(tui): MCP status panel now focusable, zoomable, and scrollable** — The MCP status panel can now be focused via Tab/Shift+Tab cycling, mouse click, and supports Ctrl+Z zoom for clean text selection. Border highlights Blue when focused (issue #d6dc3192).

### Fixed
- **fix(tui): orphan characters during streaming scroll — ratatui 0.30 upgrade** — Upgraded ratatui from 0.29 to 0.30.2 where the `Paragraph` + `.scroll()` + `Wrap` orphan character bug is confirmed fixed upstream ([ratatui#2213](https://github.com/ratatui-org/ratatui/issues/2213), [ratatui#2186](https://github.com/ratatui-org/ratatui/issues/2186)). Also upgraded crossterm from 0.28 to 0.29.0. The `Clear` widget workaround remains as an extra safety net.
- **deps: bump ratatui 0.29 → 0.30.2, crossterm 0.28 → 0.29.0.**
- **deps: bump trustee-tui to 0.1.53.**

## [0.1.96] - 2026-07-17

### Fixed
- **fix(tui): orphan characters during streaming scroll (no blinking)** — Replaced `terminal.clear()` on every `StreamDelta`/`ReasoningDelta` with a targeted `Clear` widget rendered to the output panel area before the `Paragraph` widget in both `render()` and `render_zoomed()`. This forces every cell in the output region to be marked dirty within the same frame, ensuring the diff-based renderer writes spaces for cells that previously held content from longer/wrapped lines — without causing the full-screen blinking that `terminal.clear()` introduced (trustee-tui 0.1.51).
- This is a known ratatui 0.29 bug ([ratatui#2213](https://github.com/ratatui-org/ratatui/issues/2213), [ratatui#2186](https://github.com/ratatui-org/ratatui/issues/2186)) where `Paragraph` + `.scroll()` + `Wrap { trim: false }` leaves orphan/stale characters during dynamic content changes. Confirmed fixed in ratatui 0.30-beta.
- **deps: bump trustee-tui to 0.1.51.**

## [0.1.95] - 2026-07-17

### Fixed
- **fix(tui): orphan characters during streaming scroll** — Ratatui's diff-based renderer left orphan characters when shorter lines replaced longer ones during scroll. The TUI now forces `terminal.clear()` (full repaint) whenever streaming deltas or reasoning deltas arrive, ensuring every cell is repainted.
- **deps: bump trustee-tui to 0.1.50.**

## [0.1.94] - 2026-07-17

### Added
- **feat(tui): configurable reasoning colors** — Reasoning/thinking text color is now configurable via `[tui.colors]` in trustee.toml. Defaults to `gray` + `dim` (visible on all terminals including Linux VT where `darkgray`/SGR 90 was invisible).

### Fixed
- **fix(tui): reasoning invisible on Linux virtual console** — `Color::DarkGray` (ANSI SGR 90) is unreliable on the Linux kernel VT (`fbcon`/`vgacon`) in raw mode. Default changed to `Color::Gray` (SGR 37) + `Modifier::DIM`.
- **deps: bump trustee-tui to 0.1.49.**

## [0.1.93] - 2026-07-17

### Fixed
- **fix(tui): orphan characters and jagged border boxes during streaming** — Raw `println!`/`eprintln!` calls in abk's `AgentRuntime` and `CleanupManager` bypassed the TUI mode flag and wrote directly to stdout while ratatui held the terminal in raw/alternate-screen mode. All occurrences now route through `tee_println()` or check `is_tui_mode()`.
- **fix(tui): handle terminal resize events** — `Event::Resize` was silently dropped, leaving stale buffer dimensions. The TUI now calls `terminal.clear()` before the next draw when a resize is detected.
- **deps: bump abk to 0.7.9.**
- **deps: bump trustee-tui to 0.1.48.**

## [0.1.92] - 2026-07-17

### Fixed
- **deps: bump abk to 0.7.8** — fixes critical bug where all tool outputs (bash, read,
  write) were sent to the LLM as empty strings. The native OpenAI provider now correctly
  extracts content from `ContentBlock::ToolResult` blocks in tool-role messages.
- **deps: bump trustee-tui to 0.1.47.**

## [0.1.91] - 2026-07-17

### Changed
- **feat: make WASM fully optional** — `cargo build --features tui` now produces a
  native-only build with no wasmtime dependency. Use `cargo build --features tui,wasm`
  to enable WASM extensions (provider + lifecycle). Removed `extension` from default
  abk features in trustee and trustee-tui; added `wasm` feature to forward to `abk/wasm`.
- **deps: bump abk to 0.7.7** — WASM is now opt-in via abk's `wasm` feature.
- **deps: bump trustee-tui to 0.1.46** — removes `extension` from abk features.

## [0.1.90] - 2026-07-17

### Changed
- **deps: bump abk to 0.7.6** — adds native Rust OpenAI provider (`OpenAIProvider`)
  that works without wasmtime. `LLM_PROVIDER=openai-unofficial` (or unset) now uses
  the native provider; `LLM_PROVIDER=openai-unofficial-wasm` uses the WASM extension.
  The `provider` feature no longer requires wasmtime; a new `provider-wasm` feature
  gates it. Also bumps trustee-tui to 0.1.45.
- **refactor(extensions): rename `openai-unofficial` to `openai-unofficial-wasm`** —
  directory and extension ID updated to reflect WASM-based nature.

## [0.1.89] - 2026-07-08

### Changed
- **deps: bump abk to 0.7.5** — checkpoint storage optimization: eliminates
  per-iteration `_agent.json` and `_metadata.json` duplicate files. Agent state
  is now written once as `session_agent.json`; metadata lives in `checkpoints.json`
  index. Reduces a 99-iteration session from 299 files to 101. Fully backward
  compatible with old sessions and all storage modes (Local, DocumentDB, Mirror)
  (task #a1465c3d).

## [0.1.88] - 2026-07-05

### Fixed
- **deps: bump trustee-upgrade to 0.1.2** — adds `aarch64-pc-windows-msvc` target
  triple to `current_target_triple()`, fixing `compile_error!` on Windows ARM64
  builds (issue #46eeec6b).

## [0.1.87] - 2026-07-05

### Fixed
- **deps: bump trustee-tui to 0.1.43** — `HandoffCaptureSink` now captures
  `ReasoningChunk` events. Thinking-capable models that deliver their entire
  briefing through reasoning/thinking tokens no longer produce "briefing
  unavailable". Text chunks take priority; reasoning is used as fallback
  (issue #63ad71c8).

## [0.1.86] - 2026-07-05

### Fixed
- **fix(resume): `resume -i` hang on Windows** — added defensive
  `crossterm::terminal::disable_raw_mode()` on the CLI path to handle
  terminals left in raw mode by improperly terminated TUI sessions
  (issue #2dd0cbb2).
- **deps: bump abk to 0.7.4** — `read_line` now performs blocking stdin
  read in a dedicated OS thread to avoid tokio/IOCP conflict on Windows.
  `tee_println` now flushes stdout explicitly for reliable console output
  on Windows ConPTY.

## [0.1.85] - 2026-06-30

### Added
- **feat(upgrade): `trustee upgrade` subcommand** — new `trustee-upgrade` crate
  that checks GitHub releases, downloads the correct platform binary, verifies
  SHA-256, and performs an atomic binary replacement. Supports `--check`,
  `--force`, `--dry-run`, `--version-target`, `--repo`, and `--prerelease` flags.
  Configuration is driven by `upgrade.toml` (binary name, repo, symlink paths,
  user-agent) with user overrides at `~/.trustee/upgrade.toml`.
- **feat(config): add `upgrade` command to default config** with all CLI args.

### Changed
- **deps: add `trustee-upgrade` (path), `clap` 4.6** — upgrade tool is always
  compiled in (no feature flag needed). `trustee upgrade` is intercepted in
  `main.rs` before ABK CLI dispatch.
- **deps: reqwest 0.13 with rustls** (no native-tls/openssl dependency).

## [0.1.84] - 2026-06-30

### Changed
- **deps: bump abk to 0.7.3** — fixes MCP status panel showing `0/0 (none)` when all
  MCP servers fail. The `McpToolLoader` is now kept even when `has_tools()` returns
  false, preserving `server_statuses` so failed servers with error details are emitted
  to the TUI. Also adds a no-op stub for `emit_mcp_server_statuses()` when
  `registry-mcp` feature is disabled.

## [0.1.83] - 2026-06-28

### Added
- **feat(tui): MCP Server Status Panel** — a dedicated panel in the right column
  (below Todos) showing ✓/✗ status, server name, tool count, and truncated error
  messages for each configured MCP server. Panel height is dynamic (scales with
  server count, caps at 50% of the right column). Data flows through ABK's
  `OutputEvent::McpServerStatus` — ABK stays TUI-agnostic.

### Changed
- **deps: bump abk to 0.7.1** — adds `OutputEvent::McpServerStatus` variant and
  `emit_mcp_server_statuses()` on Agent.

## [0.1.82] - 2026-06-07

### Changed
- **deps: bump abk to 0.7.0** — all raw `eprintln!` calls in abk now route through
  `tee_eprintln()` which suppresses console output in TUI mode. Fixes TUI corruption
  when MCP servers timeout or authentication fails.

## [0.1.81] - 2026-06-07

### Changed
- **deps: bump abk to 0.6.3, cats to 0.1.28** — removes interactive command detector
  (false-positive kills on commands containing `password:`, `Permission denied`, etc.)

## [0.1.80] - 2026-06-07

### Changed
- **deps: bump abk to 0.6.2** — updates cats to 0.1.28, which removes the interactive
  command detector entirely. The bash tool no longer kills commands based on pattern
  matching (e.g. `password:`, `Permission denied`, `[Y/n]`). This eliminates false
  positives where legitimate commands were blocked because their output happened to
  contain these words.

## [0.1.79] - 2026-06-07

### Added
- **feat(mcp): interactive OAuth browser login (PKCE)** — `trustee mcp auth <name>` now
  supports browser-based login with stored tokens and automatic refresh. New `interactive`
  credential type for MCP servers.

## [0.1.69] - 2026-06-08

### Changed
- **deps: bump abk to 0.5.51** — strips Windows UNC prefix (`\\?\`) from
  canonicalized paths before storing and comparing. Fixes `trustee resume`
  not recognizing the current project when the checkpoint was created on
  Windows. Also handles existing checkpoints that already have the prefix.

## [0.1.67] - 2026-06-07

### Fixed
- **fix(config): use USERPROFILE on Windows when HOME is not set** — `get_config_paths()`
  now falls back `HOME` → `USERPROFILE` → `"."` so the config file
  `~/.trustee/config/trustee.toml` is found correctly when opening a terminal directly
  on Windows (where HOME is typically unset). Previously it fell back to `"."`, looking
  for `.\\.trustee\\config\\trustee.toml` in the current directory and failing.

### Changed
- **deps: bump abk to 0.5.48** — all 9 HOME lookups in abk now fall back to USERPROFILE
  on Windows, fixing checkpoint storage, resume tracker, config, and provider factory.

## [0.1.65] - 2026-06-07

### Changed
- **deps: bump abk to 0.5.46** — bash tool (cats) and executor now use PowerShell
  instead of CMD on Windows, fixing `%` expansion, quote mangling, and single-quote
  issues. Linux/macOS behavior is unchanged.
