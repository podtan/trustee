# Trustee

A general-purpose agent that can morph into different specialized agents using WASM lifecycle plugins.

## Overview

Trustee is a terminal-first agent framework implemented in Rust that can dynamically adapt its behavior by loading different WASM lifecycle plugins. Unlike specialized agents that are built for specific domains, Trustee can morph into coding agents, research assistants, data analysts, or any other specialized agent type through its plugin architecture.

## Features

- **Morphable Architecture**: Load different lifecycle plugins to change agent behavior
- **WASM Plugin System**: Secure, sandboxed plugin execution using WebAssembly
- **Modular Design**: Built on composable crates (ABK, CATS, UMF)
- **Terminal-First**: Optimized for command-line usage and automation
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

## Lifecycle Plugins

Trustee's morphing capability comes from WASM lifecycle plugins that define different agent types:

- **Coding Agent**: Software development and engineering tasks
- **Research Agent**: Information gathering and analysis
- **Analysis Agent**: Data processing and insights
- **Custom Agents**: Create your own specialized agent types

### Creating Lifecycle Plugins

Lifecycle plugins are written in languages that compile to WebAssembly and implement the lifecycle interface defined in the WIT (WebAssembly Interface Types) specification.

## Architecture

Trustee is built on a modular architecture using several key crates:

- **ABK (Agent Builder Kit)**: Core agent building blocks with feature-gated modules
- **CATS (Code Agent Tool System)**: LLM-facing tools and utilities
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

```
trustee/
├── src/
│   └── main.rs          # Entry point using ABK convenience function
├── config/
│   └── trustee.toml     # Configuration file
├── providers/           # WASM provider binaries
├── lifecycles/          # WASM lifecycle plugins
├── AGENTS.md           # Development guidelines
├── CHANGELOG.md        # Version history
└── README.md           # This file
```

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

```bash
# List previous sessions
trustee sessions --list

# Resume a specific session
trustee resume --session abc123

# Resume the latest session
trustee resume --latest
```

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
