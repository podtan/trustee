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

Trustee uses a TOML configuration file located at `config/trustee.toml`. The configuration includes:

- **Agent settings**: Execution modes, timeouts, and behavior
- **CLI configuration**: Command-line interface customization
- **Checkpointing**: Session persistence and resume settings
- **LLM provider**: Endpoint and streaming configuration
- **Tool settings**: File operations, search filtering, and execution limits

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
