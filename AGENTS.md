# AGENTS.md

This file provides guidance to AI coding assistants when working with code in this repository.

## Project Overview

Trustee is a general-purpose agent that can morph into different specialized agents, implemented in Rust. It follows a **lifecycle-first workflow** where agent behavior is determined by WASM lifecycle plugins that define different agent types and their capabilities.

The project has been **modularized into composable crates**:
- **ABK** (Agent Builder Kit) - Complete modular agent building blocks with feature-gated modules
- **CATS** (Code Agent Tool System) - All LLM-facing tools (ACI - Agent-Computer Interface)
- **UMF** (Universal Message Format) - ChatML message formatting and streaming support
- **Lifecycle** - WASM plugin for agent lifecycle management and morphing capabilities

Trustee composes these crates to create a morphable agent framework that can adapt to different domains and tasks.

## Core Architecture

### Lifecycle-First Workflow

1. **Agent Morphing**: Tasks start with lifecycle classification to determine which agent type to morph into
2. **Agent Types**: Agents can morph into different specialized types (coding, research, analysis, etc.)
3. **Lifecycle Selection**: Based on classification, the appropriate lifecycle plugin is loaded
4. **Execution**: The agent follows the lifecycle plugin's instructions to complete the task

### Module Structure

Trustee is a **thin orchestration layer** that composes functionality from external crates:

#### Core Trustee Modules
- **`src/main.rs`** - Entry point using ABK's convenience function
- **`config/trustee.toml`** - Configuration file

#### External Crate Dependencies
- **`abk`** (v0.1.24) - Agent Builder Kit providing feature-gated modules:
  - `agent` - Complete agent implementation with orchestration and tool execution
  - `cli` - Command-line interface utilities and formatting
  - `executor` - Command execution with timeout and validation
  - `lifecycle` - WASM lifecycle plugin integration
  - `orchestration` - Workflow coordination and session management
  - `checkpoint` - Session checkpointing and resume capabilities
  - `config` - TOML configuration loading from `config/trustee.toml`
  - `observability` - Logging and monitoring
  - `provider` - LLM provider abstraction with WASM support
- **`cats`** (v0.1.2) - Code Agent Tool System providing all LLM-facing tools
- **`umf`** (v0.1.3) - Universal Message Format providing:
  - `chatml` - ChatML message formatting for LLM conversations
  - `streaming` - Streaming response support with SSE parsing
- **`lifecycle`** - WASM plugin for lifecycle management and agent morphing
- **`providers/`** - WASM provider binaries (e.g., `tanbal/` for LLM backends)

### Tool System (CATS)

All tools are defined in the **`cats`** crate (Code Agent Tool System). Tools are **structured** and **LLM-friendly**, replacing ad-hoc bash commands.

**Tool Categories:**
- **File Navigation**: `open`, `goto`, `scroll_up`, `scroll_down` (windowed file viewing)
- **Search**: `find_file`, `search_file`, `search_dir`
- **Editing**: `create_file`, `replace_text`, `insert_text`, `delete_text`, `delete_line`, `overwrite_file`, `delete_function` (Rust-aware)
- **File Management**: `delete_path`, `move_path`, `copy_path`, `create_directory`
- **Execution**: `run_command` (safe command execution with timeout and validation)
- **Utilities**: `_state`, `count_tokens`, `filemap`, `submit`, `classify_task`

Tool registry is created via `cats::create_tool_registry()` and re-exported by trustee

### Feature-Gated Architecture (ABK)

ABK uses Cargo features to enable modular functionality:

**Core Features:**
- **`config`** - TOML configuration loading and environment variable resolution
- **`observability`** - Structured logging with file/console output
- **`checkpoint`** - Session persistence and resume capabilities
- **`provider`** - LLM provider abstraction with WASM support

**Execution Features:**
- **`executor`** - Command execution with timeout and validation
- **`orchestration`** - Workflow coordination and session management
- **`lifecycle`** - WASM lifecycle plugin integration

**High-Level Features:**
- **`cli`** - Command-line interface utilities and formatting
- **`agent`** - Complete agent implementation with all dependencies

**Composite Features:**
- **`all`** - Enables all features for complete functionality

## Common Commands

### Development

```bash
# Build (debug)
cargo build

# Build (release)
cargo build --release

# Run all tests
cargo test

# Run a specific test
cargo test <test_name>

# Run the agent (after building)
cargo run -- run "Your task description here"
```

### Environment Setup

- Create a `.env` file in the project root
- Configure provider via environment variables (Tanbal WASM provider supports multiple backends)
- **OpenAI**: `OPENAI_API_KEY=sk-xxxxx`, optional `OPENAI_DEFAULT_MODEL=gpt-4o`, optional `OPENAI_BASE_URL`
- **GitHub Copilot**: `GITHUB_TOKEN=ghu_xxxxx`, `GITHUB_MODEL=openai/gpt-4o-mini` or `anthropic/claude-sonnet-4`, optional `GITHUB_BASE_URL`
- **Anthropic**: `ANTHROPIC_AUTH_TOKEN=sk-ant-xxxxx`, optional `ANTHROPIC_MODEL=claude-3-5-sonnet-20241022`, optional `ANTHROPIC_BASE_URL`
- Set `LLM_PROVIDER=tanbal` to use the WASM provider (default)

### Configuration

- Main config: `config/trustee.toml` (loaded via **abk::config**)
- Template paths configured under `[templates]` section (templates loaded from lifecycle plugins)
- Execution settings under `[execution]` (timeout, retries, max_tokens, etc.)
- Checkpointing settings under `[checkpointing]` (session resume capability via **abk::checkpoint**)
- LLM provider settings under `[llm]` (endpoint, streaming configuration via **umf::streaming**)

## Key Design Patterns

### Checkpointing and Session Management (ABK)

The agent uses **abk::checkpoint** for session management:
- Configured via `[checkpointing]` in config
- Auto-checkpoint every N iterations (configurable)
- Resume from checkpoint if interrupted
- Stored in project-specific directories with compression
- All checkpoint logic is in the **abk** crate, trustee re-exports it

### Streaming Responses (UMF)

Streaming is handled by **umf** (Universal Message Format):
- Enabled via `enable_streaming = true` in `[llm]` config
- Real-time output for long-running LLM requests
- SSE (Server-Sent Events) parsing for OpenAI and Anthropic providers
- Message accumulation and delta handling via **umf::streaming**

### LLM Provider Abstraction (ABK)

The LLM provider system is now part of **abk::provider**:
- **Purpose**: Reusable LLM provider abstraction for any Rust agent
- **Size**: ~20,517 lines in abk v0.1.24
- **Architecture**: Trait-based with WASM support
- **Key Components**:
  - `LlmProvider` trait - Core provider interface (generate, stream, configure)
  - `ProviderFactory` - Creates providers from configuration
  - `ChatMLAdapter` - Converts messages to/from ChatML format (uses umf)
  - `ToolAdapter` - Converts between tool representations
  - `WasmProvider` - WebAssembly-based provider implementation
- **Types**: `GenerateConfig`, `GenerateResponse`, `StreamingResponse`, `ToolInvocation`, `InternalMessage`, `ToolChoice`, `ToolResult`
- **Integration**: Trustee uses `abk::provider::ProviderFactory` to create LLM providers
- **Impact**: Modularized agent building blocks with feature-gated architecture

### WASM Provider System

The agent uses a **WASM-based provider architecture**:
- **Pure WASM**: Providers are WebAssembly modules loaded at runtime via `wasmtime`
- **Tanbal Provider**: Default WASM provider supporting OpenAI, GitHub Copilot, and Anthropic backends
- **Provider Discovery**: Checks `~/.trustee/providers/` and `./providers/` for `.wasm` files
- **Environment-driven**: Configuration via environment variables (API keys, base URLs, models)
- **Multi-backend support**: Single WASM provider routes to different LLM backends
- **No recompilation needed**: Add new providers by dropping `.wasm` files in providers directory

### Lifecycle Morphing System

Trustee uses a **WASM-based lifecycle architecture** for agent morphing:
- **Lifecycle Plugins**: WASM modules that define different agent types and behaviors
- **Agent Classification**: Tasks are classified to determine which lifecycle to load
- **Dynamic Behavior**: Agent behavior changes based on loaded lifecycle plugin
- **Extensible**: New agent types can be added by creating new lifecycle WASM plugins
- **Plugin Discovery**: Checks `~/.trustee/lifecycles/` and `./lifecycles/` for `.wasm` files

### Configuration Management (ABK)

Configuration is handled by **abk::config**:
- `ConfigurationLoader` - TOML file loading and validation
- `EnvironmentLoader` - Environment variable resolution
- Hierarchical config merging (defaults → file → environment)
- Type-safe configuration structs

### Observability (ABK)

Logging is handled by **abk::observability**:
- Structured logging with context
- Log levels and filtering
- File and console output options

## Lifecycle System

Lifecycles are WASM plugins that define agent behavior:

- **Agent Types**: Different lifecycles for coding, research, analysis, etc.
- **Task Classification**: Lifecycles classify tasks and determine appropriate agent behavior
- **Template Management**: Lifecycles provide templates for different task types
- **Tool Configuration**: Lifecycles can configure which tools are available for specific agent types

Lifecycles are loaded by trustee's lifecycle system and provide the morphing capabilities that differentiate trustee from specialized agents like simpaticoder.

## Testing

### Test Organization

Tests are organized to reflect the modular architecture:

- **Trustee tests** (`tests/*.rs`): Integration tests for orchestration, CLI, and agent workflows
- **ABK tests**: Checkpoint, configuration, and observability tests (in abk crate)
- **CATS tests**: Tool execution and validation tests (in cats crate)
- **UMF tests**: Message formatting and streaming tests (in umf crate)

### Current Trustee Tests

Trustee only tests its **orchestration layer**:
- Agent workflow and session management
- CLI command processing
- WASM provider integration
- Lifecycle loading and morphing
- End-to-end task execution scenarios

**Deprecated tests** (features moved to external crates):
- Checkpoint storage tests → moved to abk crate
- Streaming/JSON parser tests → moved to umf crate
- Tool-specific tests → moved to cats crate
- Configuration tests → moved to abk crate

Use `tempfile` for isolated test environments.

## Important Workspace Notes

- This is a Cargo workspace with member: `.` (main binary)
- **External crates are separate repositories**: abk, cats, umf are published on crates.io
- **No backward compatibility**: Old checkpoint/streaming/tool tests have been removed
- **WASM plugins**: Tanbal provider maintained in separate repository, place compiled `.wasm` in `providers/tanbal/`
- **Lifecycle plugins**: Agent lifecycles maintained in separate repositories, place compiled `.wasm` in `lifecycles/`
- **Dependencies**: Trustee depends on:
  - `abk = { version = "0.1.24", features = ["config", "observability", "checkpoint", "provider", "cli", "orchestration", "agent", "executor"] }`
  - `cats = "0.1.2"`
  - `umf = { version = "0.1.3", features = ["streaming"] }`
  - `wasmtime = "25"` (for WASM plugin loading)

## Migration Notes

Trustee has been designed as a **general-purpose morphable agent framework**:

1. **Lifecycle system** → `abk::lifecycle` (provides WASM plugin loading for agent morphing)
2. **Configuration** → `abk::config`
3. **Logging** → `abk::observability`
4. **Tools** → `cats` (re-exported via `src/lib.rs`)
5. **Message formatting** → `umf::chatml` (re-exported via `src/lib.rs`)
6. **Streaming** → `umf::streaming`
7. **LLM Provider abstraction** → `abk::provider` (~20,517 lines extracted in abk v0.1.24)
8. **CLI utilities** → `abk::cli`
9. **Command execution** → `abk::executor`
10. **Agent orchestration** → `abk::orchestration`
11. **Lifecycle management** → `abk::lifecycle`
12. **Complete agent** → `abk::agent`

When working on trustee:
- **Don't add lifecycle logic** - it belongs in lifecycle WASM plugins
- **Don't add tool implementations** - they belong in cats
- **Don't add message formatting** - it belongs in umf
- **Don't add provider logic** - it belongs in abk
- **Don't add CLI utilities** - they belong in abk
- **Don't add executor logic** - it belongs in abk
- **Don't add orchestration logic** - it belongs in abk
- **Focus on orchestration** - agent workflows, CLI, WASM providers, and lifecycle management</content>
<parameter name="filePath">/home/leo/Projects/Podtan/simpaticoder/tmp/trustee/AGENTS.md