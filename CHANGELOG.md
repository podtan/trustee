# Changelog

All notable changes to this project will be documented in this file.

## [0.17.1] - 2026-09-04

### Fixed

- **Ship the `${VAR}` substitution fix** — the 0.17.0 crates.io artifact was cut (14:27) from `9a2a3c5`, an hour before the fix landed in `738055b`. Root-only bump; no member changes. See the 0.17.0 Fixed section: without substitution, web mode + local-fallback config sent the literal `${OPENAI_API_KEY}` placeholder as the LLM API key (401 on every call).

## [0.17.0] - 2026-09-04

### Added

- **Web attachments: attach button + API support (multimodal via the web/THQ surface)** (nghr workstream f64e98af). The web UI now has a 📎 attach button: pick up to 4 images (jpg/jpeg/png/gif/webp, ≤6 MiB each), preview them as removable chips (with thumbnails, also shown on the sent chat bubble), and they travel with the command as base64 — the server never touches the filesystem for these.
- **`POST /api/v1/session/command` and `POST /api/v1/sessions/{id}/command` accept `attachments: [{mime, data, filename}]`** — validated fail-closed (MIME whitelist, base64 decode check, ≤6 MiB decoded per image, ≤4 per request, `data:` URL prefix tolerated, whitespace-stripped) with descriptive 400s, then converted to umf sidecar entries and attached to the initial user turn. Both fresh-session and resume paths are multimodal. The xagent (THQ proxy) session-command route accepts the same payload, so THQ consoles can adopt it without protocol changes.
- **Scoped body limits**: the global 10 MiB limit is unchanged; only the three command routes raise to 40 MiB (`DefaultBodyLimit` per-route) — base64 inflation is 4/3 and the 4×6 MiB worst case must fit.
- Transcript now shows `📎 N image attachment(s)` on the session output when a command carries images (trustee-core).

### Crate bumps

- trustee **0.17.0** (this release), trustee-core **0.7.1→0.8.0** (Session gains `input_images`; consumed by `execute_command`), trustee-api **0.13.3→0.14.0** (CommandRequest.attachments + validation), trustee-tui **0.4.1→0.4.2**, trustee-web **0.1.21→0.1.22** (attach UI), all pinning **abk 0.18.0** (`RunOptions.images` for in-memory payloads) and **umf 0.3.0** (unchanged).

### Tests

- trustee-api 71/71 (7 new attachment-validation tests: happy path, MIME normalization/whitelist, data-URL prefix, invalid base64, over-count, over-size, serde backward-compat for old clients). Live-verified end-to-end on an isolated instance: multimodal command through the HTTP API returned a pixel-accurate image description from GLM-5.3-Flash; bad MIME/base64 rejected with 400; data-URL prefix accepted.

### Fixed

- **Web mode + local-fallback config sent literal `${VAR}` placeholders as the LLM API key** (found by the attachment live test's isolated instance, where the remote getmyconfig config was unavailable). The CLI run path calls `substitute_config_vars` before executing, but `run_web_mode` never did — with the local fallback config's `api_key = "${OPENAI_API_KEY}"`, the literal placeholder string was sent to the endpoint and every LLM call failed with 401 "token expired or incorrect" (image or not). Web mode now substitutes `${VAR}` placeholders from the loaded secrets before the server starts, mirroring the CLI. Remote-config deployments (literal keys) were never affected.

## [0.16.0] - 2026-09-04

### Added

- **`trustee run 'task text' --attach img.jpg` (repeatable)** — multimodal image attachments on the CLI run path (nghr workstream 02ce6d5e, task 68b37f99). `--attach <file>` pairs are extracted from the run arguments (before or after the task text; supported types: jpg/jpeg/png/gif/webp), loaded and base64-encoded before the first model call (fail-fast with a clear error on unsupported types or unreadable files), and attached to the initial user turn for the provider to render (OpenAI `image_url` data-URL parts on the native wire path). Session titles no longer include `--attach` flags.
- **Dev dependency wiring** (task 55a6af8e): abk `0.17.0` and umf `0.3.0` resolved via local `path` deps (root + trustee-api + trustee-core + trustee-tui) with version fields kept — `cargo publish` strips paths, so publishing works unchanged once abk/umf land on crates.io. trustee-tui needed the path dep too: its registry abk with bare `features=["cli"]` cannot compile standalone (pre-existing landmine; it previously rode root's feature unification).

### Unchanged

- tui/web/xagent (THQ-dispatch) paths are text-only for now — THQ command dispatch passes an empty attachment list. WASM (tanbal) provider: image blocks now reach `format_request_from_json` input (verified in abk); tanbal-side wire rendering is a flagged follow-up, not a blocker (native OpenAI is the default wire path).
- Crate bumps beyond the root: **trustee-core `0.7.0`→`0.7.1`** (session.rs passes the new `attachments` param to abk's `run_task_from_raw_config` — required for abk 0.17.0 compat), **trustee-api `0.13.2`→`0.13.3`** and **trustee-tui `0.4.0`→`0.4.1`** (dependency-compat republishes: their published manifests pin abk `0.16.1`, which conflicts with root's `0.17.0` at crates.io resolution; no API surface change). The earlier plan note "trustee-api stays 0.13.2" does not survive crates.io version resolution — recorded here as the deviation.

## [0.15.2] - 2026-09-03

### Fixed

- **Agents can now terminate their own sessions from the THQ agent console (Terminate button)** — the console dispatches AS the agent (torpi registers trustees at their `/xagent/{agent}` advertise URL), so every console call lands with the agent's minted `role=agent` Bearer; the Cedar policy granted the agent working set every session action EXCEPT `DeleteSession` (a fail-closed deferral from the agents-as-users rollout, "revisit at task-F cutover"), so Terminate returned 403 while New Session worked. `DeleteSession` is now granted to the agent role: containment holds because an agent token scopes every session route to her OWN namespace — the grant is self-service cleanup of her own sessions and nothing more. The agent-console flow (New Session ✓ / Terminate ✗ → both ✓) is the task-F evidence the policy asked for.
- `/api/v1/health` now reports the BIN's version (`trustee --version`) instead of trustee-api's crate version — the number torpi's THQ registry and the agent console menu display as THE trustee version. Crate bump: api `0.13.1`→`0.13.2`. Tests: 64/64 (the agent delete test flipped from deny to allow with the containment rationale recorded inline).

## [0.15.1] - 2026-09-01

### Added

- **Trustee version shown in the web UI's burger menu (☰)** — a muted footer row (`trustee v0.15.1`) at the bottom of the app menu. The bin injects its own version at startup (`trustee_api::set_bin_version`, what `trustee --version` prints), and `serve_index` replaces the `__TRUSTEE_VERSION__` placeholder at serve time — so the UI always shows the running product version, never trustee-api's crate version, never a stale hardcode. Crate bumps: web `0.1.20`→`0.1.21`, api `0.13.0`→`0.13.1` (additive), root `0.15.0`→`0.15.1`. Tests: 64/64 (new: serve-time placeholder replacement — a literal `v__TRUSTEE_VERSION__` in served HTML is a test failure).

## [0.15.0] - 2026-09-01

### Changed

- **BREAKING: the hardcoded service-token key scan is REMOVED** — `[thq].service_token` (introduced in 0.14.2) is now the ONLY credential source. The legacy fallback list (`THQ_SERVICE_TOKEN`, `FAME_SERVICE_TOKEN`, `FARZAN_SERVICE_ACCOUNT`, `KANIDM_SERVICE_TOKEN`) is gone: keeping it meant an agent with an undeclared credential stayed silently invisible — the exact disease issue 8e0a1215 was filed to kill, just wearing a compatibility hat. **ACTION REQUIRED:** every dispatchable agent-user MUST have `service_token = "${YOUR_KEY}"` in her per-user overlay `[thq]` section before restarting on this version. Agents declaring nothing → loud boot ERROR (naming the overlay file), excluded from the dispatch table; declared-but-unresolved → loud boot ERROR naming agent + variable; THQ registration continues in both cases so the UI still shows her. Farewell, `FARZAN_SERVICE_ACCOUNT` — a personal name never belonged in the open-source binary. Crate bump: api `0.12.2`→`0.13.0` (behavior change → minor). Tests: 63/63 (legacy-scan tests replaced by Undeclared-outcome coverage: blessed-four keys in `.env` no longer conjure a credential).

## [0.14.2] - 2026-09-01

### Fixed

- **Per-agent service token is now config-declared, not code-guessed** (broke Paydar's live 0.10.0→0.14.1 agents-as-users migration): the boot-time dispatch capture scanned a hardcoded priority list of four `.env` keys (`THQ_SERVICE_TOKEN`, `FAME_SERVICE_TOKEN`, `FARZAN_SERVICE_ACCOUNT`, `KANIDM_SERVICE_TOKEN`) — a real key outside the blessed four (`PAYDAR_SERVICE_ACCOUNT`) was silently invisible → `entry.service_token=None` → every THQ dispatch 502'd two layers later with an unparseable non-JSON error in the THQ UI. Agents now DECLARE their credential: `[thq].service_token = "${ANY_KEY_NAME}"` in the per-user overlay, resolved from the agent's `.env` at boot (bare key name accepted too). Declared wins over the legacy list; absent → legacy scan unchanged (the 4 existing agents need zero config changes); declared-but-unresolved → LOUD boot ERROR naming agent + variable, agent NOT dispatchable — never a silent skip. `FARZAN_SERVICE_ACCOUNT` demoted to legacy-only (personal name out of the blessed path). Crate bump: api `0.12.1`→`0.12.2`. Tests: 64/64 (new: 7 declared-credential tests incl. the Paydar case — custom key name resolves with zero source changes).

## [0.14.1] - 2026-08-31

### Fixed

- **INFO flood from service-issuer fallback validation removed** — polling clients (torpi/THQ service Bearer) paid a double validation + INFO line per request because fallback results were never cached. Validation results now cache per token (SHA-256[:16] key, TTL min(exp−120s, now+300s), cap 1024); one INFO on first sight of a (sub, issuer) pair, DEBUG thereafter, exhaustion still warns. AUTHN-only — Cedar authorization stays per-request. Verified live on the production VM: continuous ~5s flood replaced by 3 one-time first-sight lines; repeated polls silent; idle agents produce no lines. Crate bump: api `0.12.0`→`0.12.1`. Tests: 57/57 (new: 6 validation-cache tests).

## [0.14.0] - 2026-08-31

### Fixed

- **Hardcoded platform persona removed from dispatched sessions** — the xagent THQ-dispatch create wrapper no longer synthesizes "You are {agent}, an agent on the Tanbal platform." for sessions without an explicit identity. That string baked the platform's identity into the open-source binary and bypassed the user-configurable `system_template` feature. Dispatched sessions now resolve their persona like every other session: agent's own per-user overlay → shared config default → abk neutral fallback. Explicit caller identity still wins.

### Added

- **Per-agent personas via overlay config** — the user-overlay allowlist now admits the `[lifecycle]` section, so each agent's `~/.trustee/users/<hash>/config/trustee.toml` can carry her own `[lifecycle].system_template` (her identity). The WASM extension switch `[lifecycle].enabled` is NOT user-settable: it is stripped from overlays (loud INFO) before the merge — agents own their persona, not extension control.

Crate bumps: api `0.11.2`���`0.12.0` (feat), root `0.13.2`→`0.14.0` (angular: feat → minor). Tests: 67/67 (new: lifecycle-overlay allowlist guard + enabled-strip; xagent create-without-identity contract). fmt flat at 272.

## [0.13.2] - 2026-08-30

### Fixed

- **Agent tokens minted on the service vhost failed inner validation** (the second half of the same split-brain): Kanidm stamps tokens with the vhost they were minted on — exchange must happen on the service issuer (0.13.1), and the RESULTING token carries that issuer, so `check_auth` must also accept it. Primary validation still runs against the `[oidc]` issuer first; on failure, validation retries against the boot-time service-issuer candidates (collected from agent-user overlays + shared `[mcp.credentials.*]`, deduped, never skipping issuer checks — same IdP, same keys, no trust expansion). Fallback hits log at INFO with the issuer used.
- `fill_userinfo_fields` now enriches against the issuer the token was validated on (name/email for service-vhost tokens would otherwise silently fail).
- farzan dispatch entry: the PM agent's per-user `.env` is restored so her THQ entry carries a service token.

## [0.13.1] - 2026-08-30

### Fixed

- **xagent token exchange used the wrong issuer** (observed live: `invalid_request` on every dispatch under 0.13.0): Kanidm accepts a token exchange only on the origin the token was minted for — the agent's escrowed token exchanges 200 on its own credential's vhost and 400 elsewhere. The exchange now takes its issuer from the agent's OWN overlay credential (`[mcp.credentials.*].issuer_url`, captured into the dispatch entry at boot), falling back to the shared config's first `service-account` credential issuer, then the `[oidc]` issuer. Diagnosis note: the `[oidc]` auth issuer host serves logins/validation fine — only exchange is origin-bound.

## [0.13.0] - 2026-08-30

### Added

- **Per-agent THQ dispatch surface (16F — `/xagent/{agent}/api/v1/...`)**: THQ-proxied sessions now run AS the agent-user instead of the caller. Mechanism: the boot-time 16E dispatch table (`[thq].agent_name → {user_key=sub, service_token}`) resolves `{agent}`; the caller is gate-checked (human admin — agents can never dispatch agents); the agent's own service token is exchanged for a short-lived `role=agent` Bearer (RFC 8693, 60s-buffered cache); the inner standard handler then authenticates and Cedar-gates **as the agent** — session bucket, per-user home, MCP loader (her own fame, never the caller's tools), and the full per-action agent matrix all resolve to her namespace. Zero handler duplication. Sessions created through the surface without an explicit identity get a minimal default ("You are {name}, an agent on the Tanbal platform.") until charters are drafted.
- **THQ wiring**: point each agent-user's `[thq].advertise_url` at `https://<host>:<port>/xagent/<agent_name>` — torpi appends `/api/v1/...` to the advertised origin, so no torpi change is required. Unknown agents 404 on the health probe (THQ marks them offline — correct for removed entries).

## [0.12.0] - 2026-08-29

### Added

- **THQ per-agent-user registration (16E — agents-as-users)**: one trustee process registers EVERY agent-user with a per-user `[thq]` section (`~/.trustee/users/{hash}/config/trustee.toml`) as its own agent in Torpi THQ — stable per-user ids (`users/{hash}/agent_id`), identity Bearer from that user's `.env` (attribution; the THQ registration route remains open), and heartbeats whose `status` reflects live session state (`idle`/`running`, keyed by the THQ `owner_id` = the agent's `sub`). The process-level `[thq]` is now ONLY a legacy single-registration fallback used when no per-user entries exist: legacy installs keep their exact behavior; agents-as-users installs never double-register a machine entry.
- Overlay allowlist accepts `[thq]` (inert in session config; prevents a false "dropping non-allowlisted section" warn on every dispatch for agent users).

### Changed

- **Agent principals are keyed by `sub` — always (16E pin on 16D)**: the JWT `user_key` rule `preferred_username || sub` now applies to HUMANS only. Agents (role=agent) are pinned to `sub` for the lifetime of the namespace, so a future token-scope change (e.g. adding `profile` to the Kanidm exchange) can never re-home an agent's `users/{hash}/` namespace.

### Fixed

- **Loud userinfo-enrichment failure** (2de5d1eb): enrichment errors were swallowed (`let _ =`) at token validation, producing a silent role-less principal that Cedar then fail-closed DENIED with `matched policies: []` and no diagnosis. Failures now log at ERROR with the consequence stated.

## [0.11.0] - 2026-08-29

### ⚠ UPGRADE REQUIREMENT — READ BEFORE DEPLOYING

Web mode is now **fail-closed on Cedar** (nghr 645809c3). After upgrading, the web **refuses to boot** unless ONE of these is in `trustee.toml`:

- `[cedar] enabled = true` — **recommended**; the role-based policies ship embedded, no extra files needed. Roles flow automatically from Kanidm claims via userinfo enrichment (admin | user | service | agent per the pdt-api-* / tocpi-services group maps).
- `[cedar] allow_disabled = true` — explicit per-environment opt-out preserving the old identity-only posture.

Additionally, a `[cedar] enabled = true` whose policy/schema fails to initialize now **hard-exits at boot** (previously it silently disabled authorization — the fame v0.1.0–0.1.1 bug class).

### Breaking

- **JWT principal user_key pinned to `preferred_username || sub`** (16D, never email). Human web users were previously keyed by JWT `sub`: on first login after upgrading, the namespace hash changes and prior checkpoints/history under the old hash are orphaned (not deleted). Dev-mode keys unchanged.
- Single Cedar `Action::"Access"` replaced by 13 per-action permits. Deployments with a filesystem policy override (`[cedar] policy_path`) referencing `Action::"Access"` will **deny everything** — update override files to the shipped action names.

### Added

- **Cedar per-action authorization** — every protected route (22 HTTP + 3 MCP-credential endpoints) now evaluates a route-specific action against role policies: `admin` = everything; `user` = full session management; `agent` = the agents-as-users working set **except DeleteSession** (fail-closed start, revisit at cutover); `service` = read-only; missing/unknown role = denied. Schema (`trustee_schema.cedarschema`) + policy (`trustee_default.cedar`) ship embedded via `include_str!`; filesystem overrides remain supported.
- **Agent principals** (16D): `PrincipalKind { Human, Agent }` classified from the enriched `role` claim (string or array); `AuthUser.role`/`AuthUser.kind`; dev-mode agent tokens `dev:agent:<name>` → `agent-<name>` namespace (gated by `local_dev_mode`) so per-user cache/THQ E2E runs without Kanidm accounts.
- `scripts/check_policies.sh` — cedar CLI validate + 9 runtime-faithful allow/deny controls as a release gate (mirrors fame's post-v0.1.2 workflow).

Crate bumps: api `0.8.0`→`0.9.0` (auth behavior = minor under 0.x), root `0.10.0`→`0.11.0` (core 0.7.0 / tui 0.4.0 unchanged). Tests: 51/51 (+6 Cedar enforcement tests incl. boot-decision fail-closed; +10 16D principal tests; +1 agent-namespace isolation). fmt flat at 272. Policy gate: `scripts/check_policies.sh` PASSED.

## [0.10.0] - 2026-08-29

### Added

- **Per-user `McpToolLoader` cache** (Agents-as-Users step C, nghr 1247ab00). Trustee previously constructed an abk `Agent` per task, and every construction connected SSE + re-discovered tools against every configured MCP server — reconnect churn on every dispatch. `ServerState` now keeps one loader per user (`mcp_loaders`, keyed by user hash, never the raw key): `get_or_build_mcp_loader()` resolves the effective config (shared + allowlist-filtered overlay + `${VAR}` substitution), fingerprints the `[mcp]` section by content (SHA-256; mtime alone insufficient since overlays are rewritten in place), single-flights builds per user, and hands out `Arc` clones on hit. Users whose effective config has MCP disabled cache a `None` marker (abk `McpSource::Prebuilt(None)` semantics). Failed builds cache a degraded entry with a 30 s retry backoff and fail loud on that user's next dispatch only — other users and boot are unaffected. One INFO line per build (`MCP loader built for user <hash8>: servers=[…] total_tools=N`) is the parity evidence for the migration task. Loader injection flows `Session.mcp_loader → RunContext.with_mcp_loader → abk McpSource::Prebuilt` (abk 0.16 API): Agent construction performs zero MCP network I/O.

### Fixed

- **Dead token-store injection gate** — `trustee-core` gated the `RunContext` token-store injection on feature `registry-mcp-token` but never declared that feature, so the gate was permanently false (flagged all along by the `unexpected_cfgs` lint and an `unused variable: token_store` warning). The feature is now declared and enabled by api/root, activating per-user token-store injection in web mode.

### Changed

- Pins move to pick up the MCP fixes: abk `0.15.0` → `0.16.1` (Arc-shared loader + prebuilt injection + single retry-on-401), pep lock `0.5.5` → `0.5.6` (service-account token cache honors the real JWT `exp` — the root-cause fix for the ~15-min Cedar fail-closed windows, nghr 199c4801/849e7528).

Crate bumps: core `0.6.24`→`0.7.0` (new public `Session` field = minor), api `0.7.30`→`0.8.0`, tui `0.3.16`→`0.4.0` (core pin), root `0.9.32`→`0.10.0` (0.x convention: step C is feature work; the pins-only-vs-full-ship call was delegated to Farzan). Tests: 34/34 (+4 loader-cache guards: disabled-marker caching, 5-way single-flight, fingerprint-change rebuild with old-`Arc` validity, degraded isolation + backoff). Warnings at parity — core improved 2→0 (both dead-gate warnings eliminated by the fix). fmt complaints flat at 272.

## [0.9.32] - 2026-08-29

### Changed (hardening)

- **Per-user config overlay allowlist** — `merge_user_config()` now merges only allowlisted top-level sections from `~/.trustee/users/{hash}/config/trustee.toml`: `[mcp]` always, and `[llm]` only when the instance opts in via the new `[users].allow_llm_overlay` knob (default `false`). Any other section in a user overlay (`[server]`, `[auth]`, `[storage]`, `[web]`, …) is dropped with a loud `warn!`, the user identified by an 8-char hash prefix only (keys may be emails). Framing: this is predictability hardening, NOT a security fix — process-level sections are boot-time only and were never re-read from session config, but a stray user overlay could silently rewrite instance-shaped knobs for that user's sessions (e.g. a `[logging]` or `[checkpointing]` override a teammate left in a shared home dir). An overlay whose sections are all non-allowlisted is now a no-op (returns `None`) instead of pointlessly re-serializing the shared config.
- **Single user-hash function** — the SHA-256[:8-bytes-BE-hex] user→directory hash now has one definition, `trustee_core::user_hash()`. All three former inline copies delegate to it: `ServerState::get_user_home_dir()` and `apply_user_isolation()` (web path, hashing the auth user_key) and the CLI binary's `compute_user_hash()` (hashing `$USER`/`$USERNAME`, input preserved). Two input domains, one algorithm — the web and CLI key spaces can no longer drift apart. Dependency moves: `trustee-core` gains `sha2` (already in the workspace lock graph); the root binary depends on `trustee-core` unconditionally (the `web` feature no longer needs to enable it) and drops its now-unused direct `sha2` dependency.

Crate bumps: core `0.6.23`→`0.6.24`, api `0.7.29`→`0.7.30`, tui `0.3.15`→`0.3.16` (tui ships the core `0.6.24` pin; root's stale `trustee-tui` pin `0.3.14` aligned to `0.3.16`). Tests: 4 new hash tests in core (known vector, determinism, 16-hex format + input sensitivity, full-digest cross-check) and 5 new overlay tests in api (allowlist structural byte-compare, no-overlay no-op, all-dropped no-op, `[llm]` knob both ways, consolidated-hash consistency).

## [0.9.31] - 2026-08-26

### Changed (deps)

- **abk 0.14.5 → 0.15.0 (BREAKING in abk)** — abk removed its dead orchestration paths: the deprecated trait-based `AgentSession` (including the history→request conversion that silently dropped assistant `tool_calls`) and the unused `AgentRuntime` "Simple Orchestration" (nghr 9f84f51d). No trustee code used either path — all entry points (CLI/TUI/web) run `Agent` + `agent_orchestration` — so this is a dependency-only change with zero behavioral impact.
- **umf requirement aligned to 0.2.7** — the manifest now declares the version the 0.9.30 release actually shipped with (`Cargo.lock` already resolved 0.2.7); closes the manifest/lockfile/changelog mismatch.
- Packaging hygiene: `HANDOFF*.md` added to `.gitignore` so working handoff notes can never ship in a published crate tarball again.

Crate bumps: core `0.6.22`→`0.6.23`, tui `0.3.14`→`0.3.15`, api `0.7.28`→`0.7.29`.

## [0.9.30] - 2026-08-25

### Changed (deps)

- **abk 0.14.4 → 0.14.5, umf 0.2.6 → 0.2.7** — assistant `reasoning_content` (thinking) is now preserved end-to-end in conversation history (nghr 1494b6fe follow-up). Previously the reasoning was captured from responses but dropped when re-serializing history for the next request; under the NInfer engine (`--preserve-thinking`) the re-rendered prompt then diverged from the resident prefix on every assistant message, so the engine fell back to `restore_turn_checkpoint` and re-prefilled the entire previous turn on the first call after each new user message (15–22s TTFT at 40–70k context). With reasoning round-tripped, the rendered prompt is byte-stable and the engine can append at its frontier. Crate bumps: core `0.6.21`→`0.6.22`, tui `0.3.13`→`0.3.14`, api `0.7.27`→`0.7.28`.

## [0.9.29] - 2026-08-25

### Changed (deps)

- **abk 0.14.3 → 0.14.4, cats 0.1.29 → 0.1.30** — deterministic `tools` array for prefix-cache reuse (nghr 1494b6fe). The OpenAI request's `tools` array was ordered by cats' `HashMap` iteration, which is per-process random in Rust: two agent runs sent the same 98 tools in a different order, changing the first system-prompt tokens and forcing a full prefill (cache miss, ~90s TTFT on qwen3.8-27b) on the first call of every run. `cats::ToolRegistry::list_tools()/get_all_schemas()` now return sorted names and `abk::provider::openai::tools::tools_to_openai()` sorts before serializing, so the `tools` payload is byte-identical across runs and processes. All three abk pins bumped (root `0.14.3`, `trustee-core` `0.14.3`, `trustee-tui` `0.14.3` → `0.14.4`); crate bumps: core `0.6.20`→`0.6.21`, tui `0.3.12`→`0.3.13`, api `0.7.26`→`0.7.27` (api re-exports core).

## [0.9.28] - 2026-08-24

### Fixed

- **Persian/non-ASCII first command no longer kills the session** (nghr 811ed903, core 0.6.20 / api 0.7.26). Auto-derived session names sliced the first command by raw byte index (`&command[..77]`, `trustee-core/src/session.rs:439`); Persian/Arabic (2-byte), CJK and emoji (3–4-byte) characters made byte 77 land mid-character → `panic: byte index is not a char boundary` in a `tokio-rt-worker` → poisoned `Arc<Mutex<Session>>` → the session stopped responding until server restart. Replaced with `truncate_session_name` (char-boundary-safe, first tests in trustee-core: 8/8). Pure-ASCII behavior is byte-for-byte identical to before (≤ 80 bytes/chars unchanged; > 80 → first 77 bytes + `...`). Non-ASCII: > 80 chars → last char boundary at or before byte 77 + `...`; ≤ 80 chars now returned unchanged (previously > 80 BYTES could truncate unnecessarily). Latent since 0.6.2-era (dfb8119e, 2026-07-27).

## [0.9.27] - 2026-08-24

### Changed (deps)

- **abk 0.14.3** — Remote-only checkpoint saves are O(1) instead of O(hwm): `save_checkpoint` now verifies mainline lineage with a rolling per-checkpoint fingerprint (same identity components as the content check) instead of range-reading the entire mainline prefix, so linear saves perform zero remote prefix reads on a fingerprint hit and fall back to the range-read only on legacy/first-save or mismatch (nghr 3c0dba81). All three abk pins bumped (root `0.14.2`, `trustee-core` `0.14.2`, `trustee-tui` stale `0.14.1` → `0.14.3`).

## [0.9.26] - 2026-08-22

### Fixed

- **`trustee-api 0.7.25` — resume no longer hijacks the caller's active live session pointer** (nghr c65d5039). `POST /api/v1/sessions/{id}/resume` created a new live session and, via `state.create_session()`, unconditionally flipped the caller's per-user `active_session_id`. Any authenticated client (curl / Torpi / API script) could therefore silently displace the live session the caller was actually working in — resuming an arbitrary session B overwrote the pointer away from A. Fixed by adding an explicit `activate: bool` param to `create_session()` (private state API): `resume_session` passes `false` (the fix), while `new_session`, `POST /api/v1/sessions`, and `ensure_active_session` pass `true` (fresh-start behavior unchanged). The web UI still switches sessions client-side via `currentSessionId`, so the server pointer is no longer authoritatively overwritable by an arbitrary client. No wire-API change.

### Changed (deps)

- **trustee-api 0.7.25** — see Fixed above.

## [0.9.25] - 2026-08-22

### Added

- **`trustee sessions migrate [--prune]`** — one-shot legacy → append-only migration. Folds legacy `{NNN}_conversation.json` cumulative snapshots into `conversation.jsonl` (INDEX + content-signature matching: the same occurrence across snapshots reuses its sequence number; legitimately repeated messages keep their own entries), backfills checkpoint cursors, writes `agent_state.jsonl` (one line per checkpoint), and normalizes `checkpoints.json` entries to the current schema. Verifies every fold (cursors ≤ log length; longest blob content-equal to the log prefix); `--prune` removes the legacy artifacts; idempotent (re-run migrates/prunes 0). Validated on a full copy of the real legacy tree (26 sessions / 214 checkpoints / 1194 messages, 0 errors, 373 files pruned, content-identical) and E2E — a migrated legacy session resumes exactly like a native one.

### Fixed (via abk 0.14.2)

- Mirror-mode remote write failures no longer swallowed (retry + gap reconcile + honest `MIRRORED` reporting; Remote-only fails loudly) — nghr 450e00d4.
- Remote-only `sessions --list` and `run --resume` (CLI dropped the remote backend when a per-user home dir was set; remote checkpoint index was never loaded) — nghr 67163136.
- `sessions --delete` and checkpoint deletion now clean the remote copy too — nghr c561e911.

### Docs

- README "Session Management" rewritten for the append-only layout; `sessions migrate` documented; known-issue note for the remote delete gap superseded by the 0.14.2 fix.

## [0.9.24] - 2026-08-22

### Changed (deps)
- **abk 0.14.1** — lineage identity now includes the `tool_calls` payload. `messages_same_lineage` (the shared helper behind both the local and remote fork checks in `save_checkpoint`) omitted `ChatMessage.tool_calls`, so a fork whose ONLY divergence is the tool-call `arguments` was classified linear — the same silent-corruption family as abk 0.13.1/0.13.2 (tie reloaded the mainline prefix; outgrow appended the branch over the mainline's seqs). The new private `tool_calls_same_lineage()` compares `id` + `r#type` + `function.name` + `function.arguments`; strictly widening identity can only flip linear→fork (conservative), so no existing behavior regresses. Trustee consumes the fork logic via web/remote resume paths.
- trustee-core 0.6.18
- trustee-tui 0.3.11

## [0.9.23] - 2026-08-22

### Changed (deps)
- **abk 0.14.0** — Removal of the dead `abk::checkpoint::v2` split-file module (~1.4k LOC, never wired into the production write path) and fixes to stale storage-format documentation (the module header advertised a "V2 Storage Format" that no production code ever wrote; `save_checkpoint`'s doc advertised `session_agent.json` / per-checkpoint `{checkpoint_id}_conversation.json`, not written since abk 0.13.0). No runtime behavior changes for trustee — trustee never referenced the v2 types (verified by grep). Storage-format docs now describe the actual append-only layout (local `conversation.jsonl` / `agent_state.jsonl` / `checkpoints.json` cursor index / fork-only snapshots; remote `messages/{seq:05}.json` / `state/{seq:05}.json` path-keyed in a single collection).
- trustee-core 0.6.17
- trustee-tui 0.3.10

## [0.9.22] - 2026-08-21

### Changed (deps)
- **abk 0.13.2** — Fork lineage check: a fork that OUTGROWS the mainline (resumed from an earlier checkpoint and kept appending) was misclassified as linear by the 0.13.1 length-only heuristic and appended its branch messages over the mainline's own sequence numbers (silent corruption of the shared `conversation.jsonl`); the `total == hwm` tie also defaulted to linear. Now fork detection compares message LINEAGE (first `hwm` messages must be identical to the mainline prefix), in both local and remote storage. Trustee's web/remote resume paths consume this.
- trustee-core 0.6.16
- trustee-tui 0.3.9

## [0.9.21] - 2026-08-20

### Fixed
- **abk 0.13.1** — Divergent-resume fork fix: resuming a non-latest checkpoint and continuing from it forks the conversation; the branch was previously silently lost (the shared `conversation.jsonl` cursor pointed at the mainline). Forks are now persisted as a full `{NNN}_conversation.json` snapshot (cursor_seq=0) in both local and remote storage, so a resumed branch reloads its exact diverged messages. Trustee's web/remote resume paths consume this.
- trustee-core 0.6.15
- trustee-tui 0.3.8

## [0.9.20] - 2026-08-18

### Changed (deps)
- **abk 0.13.0** — Checkpoint Storage Optimization: append-only `conversation.jsonl` + `agent_state.jsonl` replace the per-checkpoint `{NNN}_conversation.json` / `{NNN}_agent.json` files (O(N²) → O(N) storage); session-constant fields (task description, configuration, working directory, max iterations) move to `session_metadata.json`. Includes FIX A (true-max iteration so a legacy/mixed resume no longer restarts numbering and overwrites existing checkpoints) and the idempotent `agent_state.jsonl` append (one line per checkpoint). Each session is now exactly 4 files.
- trustee-core 0.6.14
- trustee-tui 0.3.7

## [0.9.19] - 2026-08-17

### Fixed
- **fix(web): alternative provider's default model was unselectable** — Alternative provider model cards passed `isDefault = (model === alt.default_model)`, and `selectModel` nulls the override for `isDefault`, so a provider whose only model is its default (e.g. unsloth `Qwen3.8-27B-Ridge-GGUF`) was a silent no-op on click. Now only the ACTIVE provider's default clears the override; alternative providers send the literal model string, resolved server-side by `inject_model` (exact-string match). Verified end-to-end against a tunneled llama-server.

### Changed (deps)
- trustee-web 0.1.20

## [0.9.18] - 2026-08-15

### Fixed
- **fix(tui): TUI 401 on every LLM call — `${VAR}` placeholders never substituted** — `substitute_config_vars()` now runs in `run_tui_mode()` and `run_resume_tui_mode()`. The TUI was sending the literal `${OPENAI_API_KEY}` string as the bearer token (401 on every call) because ABK skips env-var injection when a `RunContext` is present. Verified live via pty.
- **Handoff briefing prompt now requires a "Session Title: <≤50 chars>" first line** — so the downstream title generator and the pre-LLM fallback name both see a high-quality summary (wording change only).

### Changed (deps)
- trustee-core 0.6.13

## [0.9.17] - 2026-08-15

### Fixed
- **Fix 6 post-handoff bugs** (session identity rotation, transcript wipe, stale-resume race, re-handoff loop, card rename, WS chatter):
  - `SessionRotated {old,new}` WS event — clients no longer infer chain-identity rotation from `ResumeInfo` (Bug 1).
  - Preserve `output_lines` across handoff rotation — briefing run and later non-continuation runs of a rotated session keep the visible transcript (Bug 2).
  - Ignore stale client `session_id` while a rotation is pending its first checkpoint — the old chain cannot be resurrected (Bug 3).
  - `briefing_born` marker: manual + auto handoff blocked until a user command has run in the briefing chain; auto-handoff gate in `ContextTokensUpdated` (Bug 4).
  - `handoff_count` in the live session list + ↻N lineage badge on cards in both trustee-web and torpi agent detail (Bug 5).
  - `StateChanged` broadcast only on real state transitions (Bug 6).
- torpi agent-console.html adopts `SessionRotated`/`ResumeInfo` chain ids for title + history loads (persists `?history=` in URL for correct reload); `actionUrl` stays on the live registry key; fixed latent const-reassign of `sessionId`.

### Changed (deps)
- trustee-core 0.6.12
- trustee-api 0.7.23
- trustee-web 0.1.19

## [0.9.16] - 2026-08-14

### Changed
- **feat(web): Live Sessions render at the TOP of the Sessions overlay** (checkpoint sessions below). Both lists now fetch in parallel. The no-checkpoints empty state appends below any live cards instead of replacing them.
- **feat(web): Terminate button on live session cards** — softer naming than the API's "destroy" semantics; discards the in-memory session (checkpoints stay on disk). Handles 409 (running — cancel first), 404 (already gone, refreshes list), detaches the page back to the default stream if it was attached to the terminated session.

### Changed (deps)
- trustee-web 0.1.18

## [0.9.15] - 2026-08-14

### Added
- **feat(web): Live Sessions section in the Sessions overlay** — trustee-web previously only listed checkpoint (disk) sessions, with no way to see in-memory sessions. The Sessions overlay now appends a "⚡ Live Sessions" list from `GET /api/v1/sessions/live` (existing endpoint): session name, relative activity, workflow-state badge (idle/running/cancelling). "Open" attaches this page to that session via the session-scoped endpoints — WS `/api/v1/sessions/{id}/stream`, `/command`, `/cancel` — so commands target the selected session, not the legacy "active" one. History loads from the session's checkpoint. Cancelling falls back to the legacy route when not attached.

### Changed (deps)
- trustee-web 0.1.17

## [0.9.14] - 2026-08-14

### Fixed
- **fix(api): resumed sessions showed raw ID as name** — the resume handler hardcoded `Resumed: session_2026_08_14_11_00_4dc269ab` for the new in-memory session. It now looks up the checkpoint session's stored LLM-generated description (the same one shown in checkpoint-session lists) and names the resumed session `Resumed: {description}` (e.g. `Resumed: List of Planets in Order`). Falls back to the raw session ID when no description exists. Visible in Torpi/THQ Live Sessions and any client using `/api/v1/sessions/live`.

### Changed (deps)
- trustee-api 0.7.22

## [0.9.13] - 2026-08-14

### Fixed
- **fix(web): MCP status panel showed "No MCP credentials configured" for service-account/static credentials** — the `/auth/mcp/status` handler only emitted `web-session`, `web-interactive`, and `interactive` types, silently skipping `service-account` and `static` credentials. Both are now listed: `connected: true` when their token resolved non-empty (exchange happens lazily at runtime). UI labels: "Configured (auto)" / "Token missing". Pre-existing gap (not caused by the burger-menu move), verified against a real `kanidm_pdt` service-account credential serving pdt/nghr/trp.

### Changed (deps)
- trustee-api 0.7.21, trustee-web 0.1.16

## [0.9.12] - 2026-08-14

### Fixed
- **fix: agent creation crash with non-"openai-unofficial" provider names** — `[llm.provider] name = "GLM-ZAI"` (any label) previously fell into the WASM-extension dispatch path and failed with "Failed to create LLM provider" when the extension feature was disabled. `name` is now a display label ONLY; implementation selection uses the new `provider_type` field ("openai" = native default, "wasm" = extension).
- **fix: CLI 401 "token expired or incorrect"** — the CLI path never substituted `${VAR}` placeholders in the config TOML (ABK's runner skips env injection when a RunContext is present). Added `substitute_config_vars()` in main.rs, mirroring the web path's `apply_user_isolation()`. Verified working: real API calls succeed.

### Changed
- **base_url moved from .env to TOML** — `OPENAI_BASE_URL` is not a secret. It now belongs in `[llm.provider] base_url`. `BASE_URL` (new, preferred) and `OPENAI_BASE_URL` (legacy) env vars remain as fallbacks.
- **BREAKING (config schema): multi-model providers** — `[llm.provider]` now supports `models = [...]` (one endpoint, many models) plus the existing `model` (default). The `model` field in API requests is now a LITERAL model string resolved as: (1) key in `[llm.providers.*]` → provider swap, (2) offered by a named provider → swap + select, (3) otherwise set on current provider.
- **`GET /api/v1/models` response changed** — now returns `{ provider: {name, base_url, default_model, models[]}, providers: [...] }` grouped by endpoint; the web model picker groups options by provider.

### Added
- **ABK 0.12.18**: `ProviderConfig.models: Vec<String>`, `ProviderConfig.provider_type`, `effective_models()` helper.
- Web UI model picker groups by provider with base_url shown in section headers.

### Changed (deps)
- abk 0.12.18, trustee-core 0.6.11, trustee-api 0.7.20, trustee-tui 0.3.6, trustee-web 0.1.15

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
