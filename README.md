# Trustee

A general-purpose agent that can morph into different specialized agents using WASM lifecycle plugins.

## Overview

Trustee is a terminal-first agent framework implemented in Rust that can dynamically adapt its behavior by loading different WASM lifecycle plugins. Unlike specialized agents that are built for specific domains, Trustee can morph into coding agents, research assistants, data analysts, or any other specialized agent type through its plugin architecture.

## Features

- **Morphable Architecture**: Load different lifecycle plugins to change agent behavior
- **WASM Plugin System**: Secure, sandboxed plugin execution using WebAssembly
- **Modular Design**: Built on composable crates (ABK, CATS, UMF)
- **Terminal-First**: Optimized for command-line usage and automation
- **Web UI**: Browser-based interface with live WebSocket streaming (optional)
- **Session Management**: Checkpointing and resume capabilities
- **Multiple LLM Providers**: Support for OpenAI, GitHub Copilot, Anthropic, and more

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/podtan/trustee.git
cd trustee

# Build the project
cargo build --release

# Install to ~/.local/bin
cp target/release/trustee ~/.local/bin/
```

### From crates.io (when published)

```bash
cargo install trustee
```

## Quick Start

### Terminal UI (TUI)

Build with TUI support and launch with no arguments:

```bash
cargo build --release --features tui
./target/release/trustee
```

### Web UI

Build with web support and launch the API + web server:

```bash
cargo build --release --features web
./target/release/trustee web
# Open http://localhost:3000 in your browser
```

Custom bind address:

```bash
./target/release/trustee web --addr 0.0.0.0:8080
```

### CLI Mode

1. **Set up environment variables** for your LLM provider:

```bash
# For OpenAI
export OPENAI_API_KEY="sk-your-key-here"

# For GitHub Copilot
export GITHUB_TOKEN="ghu-your-token-here"

# For Anthropic
export ANTHROPIC_AUTH_TOKEN="sk-ant-your-key-here"
```

2. **Run your first task**:

```bash
trustee run "Help me analyze this codebase and suggest improvements"
```

## Configuration

Trustee uses a TOML configuration file and environment variables. Configuration is loaded from `~/.trustee/` directory.

### Configuration Files

Trustee uses two main configuration files:

1. **Config file** (`~/.trustee/config/trustee.toml`): Agent settings, execution modes, timeouts, checkpointing, LLM provider settings, and tool configuration
2. **Env file** (`~/.trustee/.env`): Environment variables and secrets (API keys, tokens, etc.)

### Environment Variables

The environment file supports the following variables:

#### Local Config File Names (Optional)

- **`TRUSTEE_CONFIG_FILE`**: Custom config file name (default: `trustee.toml`)
- **`TRUSTEE_ENV_FILE`**: Custom env file name (default: `.env`)

#### LLM Provider Configuration

- **`OPENAI_API_KEY`**: OpenAI API key
- **`OPENAI_DEFAULT_MODEL`**: Default OpenAI model (e.g., `gpt-4o`)
- **`OPENAI_BASE_URL`**: Custom OpenAI endpoint URL
- **`GITHUB_TOKEN`**: GitHub token for Copilot
- **`GITHUB_MODEL`**: Model for GitHub Copilot (e.g., `openai/gpt-4o-mini` or `anthropic/claude-sonnet-4`)
- **`GITHUB_BASE_URL`**: Custom GitHub Copilot endpoint URL
- **`ANTHROPIC_AUTH_TOKEN`**: Anthropic API key
- **`ANTHROPIC_MODEL`**: Default Anthropic model (e.g., `claude-3-5-sonnet-20241022`)
- **`ANTHROPIC_BASE_URL`**: Custom Anthropic endpoint URL
- **`LLM_PROVIDER`**: LLM provider to use (default: `tanbal`)

#### Remote Encrypted Configuration (getmyconfig)

Trustee supports loading configuration from encrypted remote storage via the `getmyconfig` library. This is useful for secure, centralized configuration management.

- **`GETMYCONFIG_ENDPOINT`**: S3-compatible storage endpoint
- **`GETMYCONFIG_ACCESS_KEY`**: Storage access key
- **`GETMYCONFIG_SECRET_KEY`**: Storage secret key
- **`GETMYCONFIG_BUCKET`**: Storage bucket name
- **`GETMYCONFIG_ENCRYPTION_KEY`**: Encryption key for decrypting config files
- **`GETMYCONFIG_REGION`**: Storage region (optional)
- **`GETMYCONFIG_CONFIG_FILE`**: Remote config file name (default: `trustee.toml.enc`)
- **`GETMYCONFIG_ENV_FILE`**: Remote env file name (default: `env.enc`)

**Example ~/.trustee/.env for remote config:**

```bash
# Remote storage configuration
GETMYCONFIG_ENDPOINT=https://your-storage.example.com
GETMYCONFIG_ACCESS_KEY=your-access-key
GETMYCONFIG_SECRET_KEY=your-secret-key
GETMYCONFIG_BUCKET=trustee-configs
GETMYCONFIG_ENCRYPTION_KEY=your-encryption-key
GETMYCONFIG_REGION=us-east-1

# Custom remote file names (optional)
GETMYCONFIG_CONFIG_FILE=my-custom-config.toml.enc
GETMYCONFIG_ENV_FILE=my-custom-secretsenv.enc

# Getmyconfig connection variables are always kept from local .env
# even when using remote config
```

**Note:** When using remote configuration, Trustee will:
1. Load local `~/.trustee/.env` first (contains getmyconfig connection params)
2. Attempt to fetch and decrypt remote config files
3. Merge remote secrets with local secrets (remote takes priority)
4. Fall back to local config if remote is unavailable

**Example ~/.trustee/.env for local config:**

```bash
# Local configuration file names (optional)
TRUSTEE_CONFIG_FILE=custom-trustee.toml
TRUSTEE_ENV_FILE=custom-secrets.env

# LLM provider configuration
OPENAI_API_KEY=sk-your-key-here
OPENAI_DEFAULT_MODEL=gpt-4o
```

## MCP Server Authentication

Trustee connects to MCP (Model Context Protocol) servers for remote tool execution. Each MCP server can use a different authentication strategy, configured in `trustee.toml`.

### Quick Reference

| Auth Type | Config `type` | Best For | Token Source |
|-----------|---------------|----------|--------------|
| Static Token | `static` | Simple API keys, dev/testing | Hardcoded or `${ENV_VAR}` |
| Service Account | `service-account` | Headless servers, CI/CD | Kanidm RFC 8693 token exchange |
| Interactive (CLI) | `interactive` | CLI/TUI personal use | `trustee mcp auth` browser PKCE flow |
| Web Session | `web-session` | Trustee-web users (reuse login) | Auto-injected from trustee-web session |
| Web Interactive | `web-interactive` | Trustee-web per-server login | Browser login via web UI |

### Configuration Overview

MCP auth is defined in two parts under `[mcp]` in `trustee.toml`:

1. **Credentials** (`[mcp.credentials.<name>]`) — reusable auth definitions
2. **Servers** (`[[mcp.servers]]`) — reference a credential by name

```toml
[mcp]
enabled = true
timeout_seconds = 30

# Define credentials (reusable across servers)
[mcp.credentials.my_auth]
type = "..."          # See below
# ... type-specific fields ...

# Servers reference credentials by name
[[mcp.servers]]
name = "pdt"
url = "https://pdt-aether.tanbal.ir"
transport = "http"
auto_init = true
credentials = "my_auth"    # References [mcp.credentials.my_auth]
```

### Auth Type 1: Static Token

Simplest method — a plain token string. Supports `${ENV_VAR}` substitution.

```toml
[mcp.credentials.pdt_static]
type = "static"
token = "${PDT_TOKEN}"
```

### Auth Type 2: Service Account

For headless server-to-server authentication. Uses a long-lived Kanidm service token and automatically exchanges it for short-lived access tokens via RFC 8693 token exchange. Tokens are refreshed automatically.

```toml
[mcp.credentials.kanidm_pdt]
type = "service-account"
service_token = "${KANIDM_SERVICE_TOKEN}"           # Long-lived, never expires
issuer_url = "https://idm.tanbal.ir/oauth2/openid/pdt-api"
client_id = "pdt-api"
audience = "pdt-api"
scope = "openid profile email groups"
```

### Auth Type 3: Interactive (CLI Browser Login)

PKCE-based OAuth2 browser login initiated from the CLI. Tokens are stored on disk and automatically refreshed.

**Config:**

```toml
[mcp.credentials.pdt_interactive]
type = "interactive"
issuer_url = "https://idm.tanbal.ir/oauth2/openid/pdt-api"
client_id = "pdt-api"
scope = "openid profile email groups"
redirect_port = 8765            # Local callback port (default: 8765)
```

**Login flow:**

```bash
# Authenticate (opens browser for OAuth login)
trustee mcp auth pdt_interactive

# Check status
trustee mcp list

# Remove credentials
trustee mcp logout pdt_interactive
```

Tokens are stored at `~/.trustee/` (via PEP's `FileTokenStore`). The token provider reads and refreshes them automatically at runtime.

### Auth Type 4: Web Session

For trustee-web deployments — reuses the logged-in user's OIDC session token. Trustee-web automatically pushes the current access token into `FileTokenStore` under the reserved name `__web_session` before each agent command. No separate login needed for MCP servers.

```toml
[mcp.credentials.pdt_web_session]
type = "web-session"

# Optional: RFC 8693 token exchange if the MCP server expects
# a different audience than the trustee-web session token
[mcp.credentials.pdt_web_session.exchange]
issuer_url = "https://idm.tanbal.ir/oauth2/openid/pdt-api"
client_id = "pdt-api"
audience = "pdt-api"
```

**How it works:**
1. User logs into trustee-web via OIDC
2. Before each agent command, trustee-web writes the session token to `__web_session`
3. The `InteractiveTokenProvider` reads it on every tool call
4. If optional `exchange` is configured, the token is exchanged for the target audience

### Auth Type 5: Web Interactive (Per-Server Browser Login from Web UI)

Like `interactive`, but the OAuth login is triggered from the trustee-web UI instead of the CLI. Uses the `/auth/mcp/login` endpoint with PKCE.

```toml
[mcp.credentials.pdt_login]
type = "web-interactive"
issuer_url = "https://idm.tanbal.ir/oauth2/openid/pdt-api"
client_id = "pdt-api"
scope = "openid profile email groups"
```

**Login flow (via trustee-web):**
1. User clicks "Connect" next to the MCP server in the web UI
2. Browser redirects to the OIDC provider
3. After login, tokens are stored via `FileTokenStore`
4. Tokens auto-refresh transparently

**Web API endpoints:**

| Method | Path | Description |
|--------|------|-------------|
| GET | `/auth/mcp/login?cred=<name>` | Initiate OAuth PKCE login for a credential |
| GET | `/auth/mcp/callback` | OIDC callback (handles code exchange + token storage) |
| GET | `/auth/mcp/status` | List all MCP credentials and their connection status |
| POST | `/auth/mcp/logout?cred=<name>` | Remove stored tokens for a credential |

### MCP CLI Commands

```bash
# List all configured MCP servers and auth status
trustee mcp list

# Register a new MCP server (interactive prompts)
trustee mcp add <name> --url <url> --auth-mode <static|service-account|interactive>

# Authenticate via browser login (interactive credentials only)
trustee mcp auth <credential_name>

# Remove stored OAuth credentials
trustee mcp logout <credential_name>

# Diagnose connection and auth for a specific server
trustee mcp debug <server_name>
```

### Environment Variable Substitution

All credential string fields support `${VAR_NAME}` syntax. The variable is resolved at startup from the environment or `~/.trustee/.env`:

```toml
[mcp.credentials.kanidm_pdt]
type = "service-account"
service_token = "${KANIDM_SERVICE_TOKEN}"     # Resolved from env
issuer_url = "https://idm.tanbal.ir/oauth2/openid/pdt-api"
client_id = "pdt-api"
audience = "pdt-api"
```

### Example: Full Configuration

```toml
[mcp]
enabled = true
timeout_seconds = 30

# --- Credentials ---

# Service account for headless auth (CI/CD, servers)
[mcp.credentials.kanidm_svc]
type = "service-account"
service_token = "${KANIDM_SERVICE_TOKEN}"
issuer_url = "https://idm.tanbal.ir/oauth2/openid/pdt-api"
client_id = "pdt-api"
audience = "pdt-api"
scope = "openid profile email groups"

# Interactive login for CLI/TUI personal use
[mcp.credentials.pdt_personal]
type = "interactive"
issuer_url = "https://idm.tanbal.ir/oauth2/openid/pdt-api"
client_id = "pdt-api"
scope = "openid profile email groups"
redirect_port = 8765

# Web session reuse for trustee-web
[mcp.credentials.web_auth]
type = "web-session"

# --- Servers ---

[[mcp.servers]]
name = "pdt"
url = "https://pdt-aether.tanbal.ir"
transport = "http"
auto_init = true
credentials = "kanidm_svc"

[[mcp.servers]]
name = "nghr"
url = "https://nghr-aether.tanbal.ir"
transport = "http"
auto_init = true
credentials = "kanidm_svc"

[[mcp.servers]]
name = "trp"
url = "https://trp-aether.tanbal.ir"
transport = "http"
auto_init = true
credentials = "kanidm_svc"
```

### Build Requirements

MCP authentication requires the `registry-mcp` feature, and token-based auth types (`interactive`, `web-session`, `web-interactive`, `service-account`) additionally require `registry-mcp-token`:

```bash
# MCP without token providers (static auth only)
cargo build --features registry-mcp

# Full MCP auth support (all credential types)
cargo build --features "registry-mcp,registry-mcp-token"

# Typical trustee build with TUI + web + full MCP
cargo build --features "tui,web,registry-mcp,registry-mcp-token"
```

### Token Storage

Tokens are stored by PEP (`FileTokenStore`) at:

```
~/.trustee/                    # or per-user home_dir
  tokens/
    <credential_name>.json     # StoredToken (access + refresh + expiry)
```

The `InteractiveTokenProvider` automatically:
- Loads tokens before each tool call
- Checks expiry and refreshes if a refresh token is available
- Returns an error if the token is expired and cannot be refreshed

## THQ Enrollment (runtime lane)

Trustee can enroll with a THQ (Trustee Head Quarters) instance as a **runtime**. Three concepts live server-side in THQ — agent identities, runtimes, and profiles (the binding) — and the runtime is the trustee **process itself**: one loop per process, reporting process-wide health (any live session → `running`, else `idle`). Zero coupling to users/agents in this lane.

### Configuration

Add a `[thq]` section to your **main** config (`~/.trustee/config/trustee.toml`) — exactly five keys, nothing else:

```toml
[thq]
thq_url = "https://thq.tanbal.ir"                       # ORIGIN only — no path, no query
instance_id = "6f635fe4-4a06-42b0-8a62-276383488a9c"   # the THQ leaf instance
advertise_url = "https://10.99.0.11:3000"               # this process's reachable URL
runtime_name = "nox"                                    # shown in THQ as "Runtime: nox"
heartbeat_interval = 30                                 # pull/state cadence (seconds)
```

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `thq_url` | Yes | — | THQ's origin (`scheme://host[:port]` only; the leaf goes in `instance_id`) |
| `instance_id` | Yes | — | The THQ leaf instance id, sent as `X-Instance-Id` on every call |
| `advertise_url` | Yes | — | This runtime's externally-reachable URL (THQ calls back to it) |
| `runtime_name` | Yes | — | The runtime's name (vocabulary: runtime, never agent) |
| `heartbeat_interval` | No | `30` | Pull/state cadence in seconds |

The section is strict: unknown keys — including registration-era ones (`torpi_url`, `agent_name`, …) — fail loudly at boot with the lane disabled. There is no compatibility translation; migrate the config.

### The enrollment lane

1. **Register** — once at boot (and only ever again after a 403), trustee POSTs `{name, advertise_url}` to `{thq_url}/api/v1/runtime/register` with the `X-Instance-Id` header. The runtime's identity is deterministic: `runtime_key = sha256(advertise_url)` — no ids pasted anywhere.
2. **Callback** — THQ POSTs `{thq_url, runtime_id, instance_id, secret}` to `{advertise_url}/thq/enroll`. Trustee verifies the payload's origin against the configured `thq_url` (rogue-THQ guard), plus instance and runtime key, then persists the secret to `~/.trustee/thq/runtime.json` (0700/0600) — process-level, never under `users/`.
3. **Pull & state** — every `heartbeat_interval`, GET `/api/v1/runtime/profiles` with `X-Runtime-Id` + `X-Runtime-Secret` (bound profiles with their identity payload are logged — wire-capture — not yet applied), and report process health per bound profile. A 403 drops the credential and re-registers: secret rotation self-heals.

### Per-user dispatch lane (unchanged)

Per-user `[thq]` overlay sections no longer drive registration. They still declare the **xagent dispatch** target for agents-as-users: `agent_name` (dispatch key), `owner_id` (the agent's Kanidm `sub`), and `service_token` (the env var holding her service credential). The old `~/.trustee/agent_id` files are obsolete in the runtime lane and ignored.

## Lifecycle Plugins

Trustee's morphing capability comes from WASM lifecycle plugins that define different agent types:

- **Coding Agent**: Software development and engineering tasks
- **Research Agent**: Information gathering and analysis
- **Analysis Agent**: Data processing and insights
- **Custom Agents**: Create your own specialized agent types

### Creating Lifecycle Plugins

Lifecycle plugins are written in languages that compile to WebAssembly and implement the lifecycle interface defined in the WIT (WebAssembly Interface Types) specification.

## Architecture

Trustee is built on a modular workspace architecture:

```
trustee/
├── Cargo.toml            # Workspace root + binary
├── src/main.rs           # Binary — dispatches: TUI / web / CLI / upgrade
├── crates/
│   ├── trustee-core/     # Shared types, Session state, workflow logic, config parsing
│   ├── trustee-tui/      # Terminal UI (ratatui + crossterm), depends on trustee-core
│   ├── trustee-api/      # REST + WebSocket server (axum), depends on trustee-core + trustee-web
│   ├── trustee-web/      # Static web frontend (rust-embed)
│   └── trustee-upgrade/  # Self-upgrade tool
├── config/
│   └── trustee_default.toml
└── CHANGELOG.md
```

**Feature flags:**

| Flag | Description |
|------|-------------|
| (none) | Bare CLI — no TUI, no web |
| `tui` | Terminal UI mode (`trustee` with no args launches TUI) |
| `web` | Web/API mode (`trustee web` starts HTTP + WebSocket server) |
| `wasm` | WASM lifecycle plugin support |

Build with multiple features: `cargo build --features tui,web`

### API Endpoints (web mode)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/health` | Health check |
| GET | `/api/v1/session` | Full session state snapshot |
| POST | `/api/v1/session/command` | Submit a command for execution |
| POST | `/api/v1/session/cancel` | Cancel running workflow |
| POST | `/api/v1/session/handoff` | Trigger session handoff |
| WS | `/api/v1/session/stream` | Live message streaming |
| GET | `/` | Web UI (embedded HTML/JS) |

All network dependencies use **rustls** (no openssl/native-tls) for static Pi/ARM builds.

### External Dependencies

- **ABK (Agent Builder Kit)**: Core agent building blocks with feature-gated modules
- **UMF (Universal Message Format)**: ChatML message formatting and streaming
- **Lifecycle Plugins**: WASM modules defining agent behavior

## Development

### Prerequisites

- Rust 1.70 or later
- Cargo package manager

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test
```

### Project Structure

See the [Architecture](#architecture) section above for the full workspace layout.

## Usage Examples

### Basic Task Execution

```bash
# Run with confirmation mode (default)
trustee run "Refactor this function to be more efficient"

# Run in YOLO mode (no confirmations)
trustee run --yolo "Update all dependencies"

# Use a specific mode
trustee run --mode human "Help me debug this issue interactively"
```

### Session Management

Sessions use an append-only storage layout (one shared `conversation.jsonl`
per session, cursor-addressed checkpoints — see the `abk` checkpoint module
docs). Storage mode is set by `[checkpointing.storage_backend] storage_mode`
in `trustee.toml`: `local`, `mirror` (local + remote), or `remote`.

```bash
# List previous sessions
trustee sessions --list

# Show a session's checkpoints
trustee sessions --show <session_id>

# Delete a session (local storage; see known-issue note below)
trustee sessions --delete <session_id>

# Resume a specific session (latest checkpoint)
trustee run --resume <session_id> "continue this task"

# Resume the latest session
trustee resume --latest
```

**Migrating legacy sessions.** Sessions created before the append-only
format store a full conversation snapshot per checkpoint
(`{NNN}_conversation.json`, `{NNN}_agent.json`, `session_agent.json`).
`trustee sessions migrate` folds them into the current layout:

```bash
# Convert legacy sessions (keeps the original files)
trustee sessions migrate

# Convert and delete the legacy artifacts afterwards
trustee sessions migrate --prune
```

The migration is idempotent (safe to re-run), verifies each folded session
(checkpoint cursors and the longest conversation snapshot are re-read and
compared after conversion), and normalizes legacy `checkpoints.json` entries
to the current schema. Resuming a migrated session works exactly like a
native one.

> **Known issue (remote storage):** `sessions --delete` currently removes
> only the local directory; remote copies under `projects/{hash}/sessions/…`
> are left behind and the session may re-appear in listings. Tracked as
> nghr c561e911-7285-4389-85d8-4b0d42c40725 (fix planned for the next
> release train alongside the remote-only fixes in
> 67163136-6ec1-4586-883d-ced3259f9177).

### Configuration Management

```bash
# Show current configuration
trustee config --show

# Edit configuration file
trustee config --edit

# Validate configuration
trustee config --validate
```

## Contributing

We welcome contributions! Please see our [contributing guidelines](CONTRIBUTING.md) for details.

### Development Setup

1. Fork the repository
2. Clone your fork: `git clone https://github.com/yourusername/trustee.git`
3. Create a feature branch: `git checkout -b feature/your-feature`
4. Make your changes and add tests
5. Run the test suite: `cargo test`
6. Submit a pull request

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Trustee builds upon several key technologies and projects:

- **ABK**: Agent Builder Kit for modular agent components
- **CATS**: Code Agent Tool System for LLM tool integration
- **UMF**: Universal Message Format for LLM communication
- **Wasmtime**: WebAssembly runtime for plugin execution
- **Tokio**: Asynchronous runtime for Rust

## Support

- **Issues**: [GitHub Issues](https://github.com/podtan/trustee/issues)
- **Discussions**: [GitHub Discussions](https://github.com/podtan/trustee/discussions)
- **Documentation**: [Wiki](https://github.com/podtan/trustee/wiki)

---

**Trustee** - Morph into the agent you need, when you need it.
