---
# UDML Component Files Summary
# Generated for Trustee Agent System

## Format Selection: YAML

**Why YAML?**
- Human-readable and editor-friendly (as opposed to JSON's verbosity or TOML's limited nesting)
- Schema validation friendly (can be converted to/from JSON for tools)
- Comment support for documentation
- Hierarchical structure matches UDML's six-domain model naturally
- Recommended in UDML specification itself

## Components Generated (13 files)

### Core Orchestration Layer
1. **abk-agent.udml.yaml** — Core agent runtime and component wiring
   - Coordinates all components
   - Defines message flow, workflow loop, tool execution
   - Central orchestration of LLM, tools, lifecycle, checkpointing

2. **abk-cli.udml.yaml** — CLI/Bootstrap layer
   - Entry point and command routing
   - Configuration loading and validation
   - Session management (run, resume, list)

### Configuration & Runtime Setup
3. **abk-config.udml.yaml** — Configuration loader
   - TOML file parsing and validation
   - Environment variable overrides
   - Configuration distribution to all components

4. **abk-provider.udml.yaml** — LLM provider factory
   - Provider abstraction trait
   - Factory pattern for creating providers
   - WASM and HTTP provider support
   - Request/response marshaling

### Lifecycle & Plugin System
5. **abk-lifecycle.udml.yaml** — WASM lifecycle templates & classification
   - Plugin loading and instantiation
   - Template management and rendering
   - Task classification for agent morphing

### Component Management
6. **abk-executor.udml.yaml** — Command executor
   - Safe sandboxed command execution
   - Timeout enforcement
   - Output capture and truncation
   - Dangerous command detection

7. **abk-checkpoint.udml.yaml** — Session persistence
   - Checkpoint save/load/resume
   - Session metadata management
   - Compression and integrity checking
   - Cleanup and retention policies

8. **abk-orchestration.udml.yaml** — Workflow coordinator
   - Multi-step workflow execution
   - Dependency resolution
   - Retry logic with backoff
   - Parallelization of independent steps

9. **abk-observability.udml.yaml** — Logging and telemetry
   - Structured event logging
   - Metrics collection
   - Distributed tracing
   - Log rotation and management

### Tool System & Messaging
10. **cats.udml.yaml** — Code Agent Tool System
    - Tool registry and definitions
    - Tool discovery and execution
    - Input validation and output formatting
    - Tool categories (file, search, edit, execution)

11. **umf.udml.yaml** — Universal Message Format
    - Internal message representation
    - ContentBlock (text, tool_use, tool_result)
    - ChatML formatting for providers
    - Streaming delta accumulation

### WASM Plugins (Runtime-Loaded)
12. **provider-wasm-tanbal.udml.yaml** — Multi-backend LLM provider
    - WASM-based request/response formatting
    - Support for OpenAI, Anthropic, GitHub Copilot
    - Streaming event parsing
    - Per-backend encapsulation

13. **lifecycle-wasm.udml.yaml** — Agent lifecycle plugin
    - Template loading and rendering
    - Task classification and reasoning
    - Agent type morphing
    - Specialized configurations per agent type

## UDML Structure in Each File

Every component follows the six-domain UDML model:

```yaml
metadata:
  # Component identification and ownership

information:
  # Data structures, schemas, state definitions
  # "What data does this component manage?"

access:
  # Query interfaces, visibility rules, boundaries
  # "How is data accessed?"

manipulation:
  # Mutation rules, creation, update, deletion
  # "How is data changed?"

extract:
  # Derivation rules, transformations, projections
  # "What new data is derived?"

movement:
  # Data flow between components, boundaries, protocols
  # "How does data travel?"

coordination:
  # Orchestration, sequencing, synchronization, retry logic
  # "How do components work together?"

dependencies:
  # Internal (other components) and external (crates)

notes:
  # Implementation guidance and design rationale
```

## Reading Guide

**Start here:**
1. `abk-agent.udml.yaml` — Understand central orchestration
2. `umf.udml.yaml` — Understand message format
3. `abk-provider.udml.yaml` — Understand LLM provider abstraction

**Then explore by concern:**
- **Bootstrapping:** `abk-cli.udml.yaml` → `abk-config.udml.yaml`
- **Execution:** `abk-orchestration.udml.yaml` → `abk-executor.udml.yaml`
- **Tools:** `cats.udml.yaml` → `abk-executor.udml.yaml`
- **Persistence:** `abk-checkpoint.udml.yaml`
- **Observability:** `abk-observability.udml.yaml`
- **Plugins:** `provider-wasm-tanbal.udml.yaml` and `lifecycle-wasm.udml.yaml`

## Key Design Insights Captured

### UDML to Trustee Mapping

| UDML Domain | Trustee Examples |
|-------------|-----------------|
| **Information** | InternalMessage (UMF), CheckpointData, ProviderConfig, ToolCall |
| **Access** | Query methods, visibility levels, auth/permission rules |
| **Manipulation** | Session creation, tool execution, checkpoint save/load |
| **Extract** | Message formatting, tool result parsing, metrics aggregation |
| **Movement** | In-process calls, WASM boundaries, HTTP to providers, file I/O |
| **Coordination** | Workflow loops, retry policies, dependency resolution, error handling |

### Tightly-Coupled Module Analysis (from issue_mapping.md)

The UDML reveals the practical concerns:

1. **Feature-vs-Ownership mismatch**: `abk[*]` feature flags vs true module boundaries → Clarified through WASM plugin boundaries
2. **Movement boundary ambiguity**: WASM plugins as explicit movement boundaries (defined)
3. **Information fragmentation**: UMF as canonical message format (centralized)
4. **Cross-cutting responsibilities**: Observability mapped through all components
5. **Implicit contracts**: Explicit WASM ABI definitions for plugins

## Future Use Cases

These UDML files enable:

- **Code generation** — Generate stubs or boilerplate from UDML specs
- **Test generation** — Derive integration test scenarios from movement/coordination
- **Documentation** — Auto-generate API docs, architecture diagrams
- **Validation** — Check implementation against UDML specification
- **Migration** — Plan refactors between architectures with clear data flow mappings
- **Monitoring** — Generate observability checks from coordination patterns

## Version Control Notes

All files are in `/data/Projects/podtan/trustee/docs/UDML/components/` directory:
- Format: YAML (human and machine-readable)
- Schema: Trustee-specific UDML format (can be formalized later)
- Lifecycle: Living documents; update alongside code changes

---

**Generated:** November 10, 2025
**Format:** YAML
**Rationale:** Human-readable, schema-friendly, hierarchical, alignment with UDML spec
