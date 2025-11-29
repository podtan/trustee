# Answers to Agent Evolution Part 3 Questions

**Analysis Date:** November 29, 2025  
**Branch:** coupled  
**Respondent:** Claude Opus 4.5

---

## Executive Summary

After reviewing the Agent Evolution documents (Parts 1-3), the hypothesis document, the current data flow analysis, and examining the source code in `tmp/` (abk, cats, umf, coder-lifecycle, tanbal-provider), I provide answers to the five questions posed in Part 3.

The core insight: **The current coupling issues stem from mixing type definitions with behavior implementations.** The WASM plugins (coder-lifecycle, tanbal-provider) already demonstrate the correct pattern—they communicate via WIT interfaces, not shared Rust types.

---

## Question 1: Where Should Types Live?

**Question:** "If I want Trustee to be modular and every module replaceable, where should I keep the types: `trustee[types]` or `abk[types]`? What about CATS and UMF — keep all types in trustee?"

### Answer: Create a Separate Types Crate

**Recommendation:** Create a new standalone crate called `trustee-types` (or `abk-types`) that contains ONLY type definitions, no behavior.

#### Evidence from Source Code

The current coupling is visible in `tmp/abk/src/config/config.rs`:

```rust
pub struct Configuration {
    // ...
    #[cfg(feature = "cli")]
    pub cli: Option<crate::cli::config::CliConfig>,  // COUPLING
}
```

And in `tmp/abk/src/provider/mod.rs`:

```rust
// Re-export streaming types from umf
pub use umf::StreamChunk;
pub use umf::{ToolCall, FunctionCall, Function, Tool};
```

ABK imports UMF types in 4+ modules, creating tight coupling.

#### The C Header Model

C solves this with header files (`.h`) that declare types and function signatures, while implementation files (`.c`) provide behavior. The analogy in Rust:

| C Concept | Rust Equivalent |
|-----------|-----------------|
| `.h` header | Types crate (interfaces, structs, enums only) |
| `.c` implementation | Feature crates (behavior using types) |
| ABI stability | Semantic versioning of types crate |

#### Kubernetes Model

Kubernetes uses `k8s.io/api` for type definitions and `k8s.io/client-go` for behavior. All controllers depend on the shared types package, not on each other.

#### PostgreSQL Model

PostgreSQL defines types in header files (`nodes.h`, `parsenodes.h`), and extensions link against the core type definitions. Extensions don't import each other's internal types.

#### Proposed Architecture

```
trustee-types/          # NEW: Types-only crate
├── Cargo.toml          # Minimal deps: serde, serde_json
├── src/
│   ├── lib.rs
│   ├── message.rs      # InternalMessage, MessageRole, ContentBlock
│   ├── tool.rs         # ToolCall, ToolResult, Tool, Function
│   ├── provider.rs     # GenerateConfig, GenerateResponse
│   ├── checkpoint.rs   # Checkpoint, SessionMetadata
│   └── config.rs       # Configuration types (no CLI coupling)

abk/                    # Depends on trustee-types
├── features: [config, provider, checkpoint, cli, agent]

umf/                    # Depends on trustee-types (or becomes part of it)
cats/                   # Depends on trustee-types
```

#### Key Principle

**Types crate should have:**
- Zero behavior (no methods, no traits with default impls)
- Only `serde` dependencies
- Stable versioning (breaking changes = major version bump)

---

## Question 2: Native Extensions in Other Languages

**Question:** "What if I want to use another programming language (not Rust) as a native extension rather than WASM?"

### Answer: Use C FFI with Stable ABI

**Recommendation:** Define a C-compatible FFI boundary using `extern "C"` functions and `#[repr(C)]` structs.

#### Options Ranked by Pragmatism

| Approach | Complexity | Performance | Safety | Polyglot Support |
|----------|------------|-------------|--------|------------------|
| WASM (current) | Low | Good | High | Excellent |
| C FFI | Medium | Excellent | Medium | Good |
| gRPC/IPC | Medium | Lower | High | Excellent |
| Shared object (.so/.dylib) | High | Excellent | Low | Medium |

#### C FFI Approach (Recommended for Performance-Critical Extensions)

```rust
// In trustee-ffi crate
#[repr(C)]
pub struct FfiMessage {
    role: *const c_char,
    content: *const c_char,
}

#[no_mangle]
pub extern "C" fn trustee_process_message(msg: *const FfiMessage) -> *mut c_char {
    // Implementation
}
```

Other languages (Zig, Mojo, Go, Python via ctypes) can call this interface.

#### What Zig/Mojo/Hare Would Use

These languages all support C FFI natively:
- **Zig**: Direct `extern fn` declarations
- **Mojo**: C interop via `external_call`
- **Hare**: `@symbol` for FFI bindings
- **Go**: `import "C"` (cgo)

#### Hybrid Approach

For maximum flexibility:
1. **Performance-critical**: C FFI (native extensions)
2. **Sandboxed/portable**: WASM (current lifecycle/provider model)
3. **Cross-process**: gRPC or Cap'n Proto (optional)

The WASM plugins already work well—keep them for provider and lifecycle plugins. Add C FFI only when WASM overhead is measurable.

---

## Question 3: Low-Hanging Fruits

**Question:** "What are the low-hanging fruits in the current architecture based on established software-engineering practices?"

### Answer: Six Immediate Improvements

Based on the hypothesis document's observations and code inspection:

#### 1. Separate `lifecycle` Feature from `agent` (Estimated: 2 hours)

**Current state:** In `tmp/abk/src/lib.rs`:
```rust
#[cfg(feature = "agent")]
pub mod lifecycle;
```

**Problem:** Cannot use lifecycle without full agent dependencies.

**Fix:** Add standalone `lifecycle` feature:
```toml
lifecycle = ["wasmtime", "wasmtime-wasi", "serde_json"]
agent = [..., "lifecycle", ...]
```

#### 2. Remove CLI Type from Config (Estimated: 3 hours)

**Current state:** In `tmp/abk/src/config/config.rs`:
```rust
#[cfg(feature = "cli")]
pub cli: Option<crate::cli::config::CliConfig>,
```

**Problem:** Config cannot compile without CLI feature present.

**Fix:** Use a generic `HashMap<String, Value>` for extension configs:
```rust
pub extra: Option<HashMap<String, serde_json::Value>>,
```

#### 3. Make UMF Types Private, Expose via Interface (Estimated: 4 hours)

**Current state:** UMF v0.2.0 already started this (types are `pub(crate)`).

**Problem:** ABK still imports UMF types directly via re-exports.

**Fix:** Define trait-based adapters in ABK that abstract over message types:
```rust
pub trait MessageFormatter {
    type Message;
    fn format(&self, msg: &Self::Message) -> String;
}
```

#### 4. Add Direct CATS/UMF Dependencies to Trustee (Estimated: 1 hour)

**Current state:** Trustee only depends on ABK, which re-exports CATS and UMF.

**Problem:** False independence—version updates require ABK rebuild.

**Fix:** Add direct dependencies in `trustee/Cargo.toml`:
```toml
cats = "0.1.2"
umf = { version = "0.2.0", features = ["streaming"] }
```

#### 5. Create Feature Independence Tests (Estimated: 4 hours)

**Current state:** No automated testing of feature independence.

**Problem:** Can't verify 22% modularity claim without tests.

**Fix:** Add CI job that builds each feature combination:
```bash
cargo build --features executor
cargo build --features observability
cargo build --features config
# etc.
```

#### 6. Extract Orchestration to Workflow Crate (Estimated: 8 hours)

**Current state:** 1,500 lines in `abk/src/orchestration/`.

**Problem:** Orchestration is too large for a "utility" feature.

**Fix:** Create `trustee-workflow` crate that depends only on `trustee-types`:
```
trustee-workflow/
├── Cargo.toml  # deps: trustee-types, tokio, async-trait
├── src/
│   ├── coordinator.rs
│   ├── runtime.rs
│   └── session.rs
```

### Priority Matrix

| Fruit | Impact | Effort | Priority |
|-------|--------|--------|----------|
| 1. Separate lifecycle | High | Low | P0 |
| 2. Remove CLI from config | High | Low | P0 |
| 3. UMF interface | Medium | Medium | P1 |
| 4. Direct deps in Trustee | Low | Very Low | P1 |
| 5. Feature tests | Medium | Medium | P1 |
| 6. Workflow crate | Medium | High | P2 |

---

## Question 4: Removing agent/orchestration from ABK

**Question:** "I'm unhappy with `abk[agent]` and `abk[orchestration]` — I was forced to create them to extract `abk[cli]`. How could I remove or migrate those concepts so the design is cleaner?"

### Answer: Invert the Dependency Direction

**Root cause:** CLI extraction failed because orchestration/agent code was tangled with CLI rendering concerns. The solution is dependency inversion.

#### Current Architecture (Problematic)

```
CLI → Agent → Orchestration → Provider → UMF
        ↓
       CATS
```

CLI depends on Agent, which pulls everything.

#### Proposed Architecture (Clean)

```
                    ┌─────────────────┐
                    │ trustee-types   │  (types only)
                    └────────┬────────┘
                             │
        ┌────────────────────┼────────────────────┐
        │                    │                    │
        ▼                    ▼                    ▼
┌───────────────┐   ┌───────────────┐   ┌───────────────┐
│ trustee-work- │   │  abk-provider │   │   abk-cli     │
│    flow       │   │  (traits)     │   │ (display)     │
└───────┬───────┘   └───────────────┘   └───────────────┘
        │
        ▼
┌───────────────────────────────────────────────────────┐
│                       TRUSTEE                          │
│  (composes workflow + provider + cli + cats + umf)    │
└───────────────────────────────────────────────────────┘
```

#### Migration Path

**Phase 1: Extract Types (Week 1)**
1. Create `trustee-types` crate
2. Move type definitions from UMF, CATS, ABK
3. Update all crates to depend on `trustee-types`

**Phase 2: Invert CLI (Week 2)**
1. Make CLI a display-only layer
2. CLI defines traits for what it needs to display
3. Agent implements CLI traits (adapter pattern)

Example:
```rust
// In abk-cli (display layer)
pub trait AgentDisplay {
    fn get_status(&self) -> String;
    fn get_iterations(&self) -> u32;
    fn get_task_description(&self) -> String;
}

// In trustee (composition layer)
impl AgentDisplay for Agent {
    // ...
}
```

**Phase 3: Move Orchestration to Trustee (Week 3)**
1. Remove `abk[orchestration]`
2. Move orchestration into `trustee-workflow` or directly into Trustee
3. ABK becomes purely utility code (config, checkpoint, executor)

**Phase 4: Remove `abk[agent]` (Week 4)**
1. Agent struct lives in Trustee, not ABK
2. ABK provides building blocks, Trustee composes them
3. Result: ABK is pure utilities, Trustee is the agent

#### Final ABK Features

After migration, ABK would have:
```toml
[features]
config = [...]       # Configuration loading
observability = [...]  # Logging
checkpoint = [...]   # Session persistence
provider = [...]     # LLM provider traits (no concrete impl)
executor = [...]     # Command execution
cli-display = [...]  # CLI formatting utilities (no commands)
```

No `agent`, no `orchestration`—those live in Trustee.

---

## Question 5: Data Flow as Core Idea

**Question:** "I think Data Flow is the core idea for composing software. UDML felt like a dead end — what are current trends and similar ideas? Is there a workable approach?"

### Answer: Data Flow Without Runtime Schemas

**Core insight:** UDML failed because it moved type definitions to runtime. The correct approach is **compile-time data flow with runtime coordination**.

#### Why UDML Failed (Per Part 2)

1. **Type safety loss**: Rust types → YAML schemas → runtime validation
2. **Performance**: Large URP documents instead of small values
3. **ESB reinvention**: Became an enterprise service bus

#### Current Trends Aligned with Data Flow

| Approach | Compile-Time Types? | Runtime Coordination? | Example |
|----------|---------------------|----------------------|---------|
| Effect Systems | Yes | Yes | ZIO (Scala), Effect-TS |
| Algebraic Effects | Yes | Yes | OCaml 5, Eff |
| Actor Model | Yes | Yes | Akka, Erlang/OTP |
| Reactive Streams | Yes | Yes | RxJava, Tokio Streams |
| Data-Oriented Design | Yes | No | ECS patterns |
| **WIT/Component Model** | **Yes** | **Yes** | **WASM Component Model** |

#### Recommended: WASM Component Model

The WASM Component Model (WIT) is the closest to what you want:

1. **Types defined in WIT** (compile-time, language-agnostic)
2. **Components communicate via interfaces** (not shared memory)
3. **No runtime schema parsing** (types are compiled into WASM modules)

You're already using this! The lifecycle and provider plugins use WIT interfaces.

**Extension:** Apply the same pattern internally within ABK:

```wit
// internal-coordinator.wit
interface coordinator {
    record task-request {
        description: string,
        context: option<string>,
    }
    
    record task-result {
        success: bool,
        output: string,
    }
    
    process-task: func(request: task-request) -> task-result;
}
```

Each ABK module could be a mini-component with WIT-defined interfaces.

#### Alternative: Rust Traits as Data Flow Contracts

For internal Rust code, traits already provide compile-time data flow contracts:

```rust
// Data flows from Provider → Formatter → Orchestrator → Tools
pub trait MessageSource {
    fn next_message(&mut self) -> Option<Message>;
}

pub trait MessageSink {
    fn accept_message(&mut self, msg: Message);
}

pub trait MessageTransformer {
    fn transform(&self, msg: Message) -> Message;
}
```

This is essentially the Iterator/Stream pattern—data flows through a pipeline of transformers.

#### Practical Recommendation

1. **Keep WASM Component Model for plugins** (lifecycle, provider)
2. **Use Rust traits for internal composition** (within ABK/Trustee)
3. **Define types in a separate crate** (trustee-types)
4. **Document data flow in architecture diagrams** (like current_data_flow.md)
5. **Avoid runtime schema validation** for hot paths

The data flow documentation you already have (`current_data_flow.md`) is valuable. Formalize it by ensuring each interface in the flow is explicit (trait or WIT interface), but don't reinvent an ESB.

---

## Summary: Action Items

| Priority | Action | Estimated Effort |
|----------|--------|------------------|
| P0 | Create `trustee-types` crate | 1 day |
| P0 | Separate `lifecycle` feature from `agent` | 2 hours |
| P0 | Remove CLI type from config struct | 3 hours |
| P1 | Add feature independence CI tests | 4 hours |
| P1 | Add direct CATS/UMF deps to Trustee | 1 hour |
| P2 | Extract orchestration to standalone crate | 1 week |
| P2 | Invert CLI dependencies (display-only) | 1 week |
| P3 | Add C FFI interface for native extensions | 2 weeks |

The WASM plugin architecture is already correct—extend that pattern internally rather than creating new abstractions.

---

## Appendix: Source Code Evidence

### UMF Type Coupling in ABK Provider

From `tmp/abk/src/provider/mod.rs`:
```rust
pub use umf::StreamChunk;
pub use umf::{ToolCall, FunctionCall, Function, Tool};
```

ABK re-exports UMF types, creating tight coupling.

### Clean WASM Plugin Example

From `tmp/coder-lifecycle/Cargo.toml`:
```toml
[dependencies]
wit-bindgen = "0.39.0"
# No ABK, no UMF, no CATS dependencies!
```

From `tmp/tanbal-provider/Cargo.toml`:
```toml
[dependencies]
wit-bindgen = "0.30"
# No ABK, no UMF, no CATS dependencies!
```

These plugins demonstrate the correct pattern—communication via WIT interfaces, not Rust type imports.

### Agent Dependencies

From `tmp/abk/Cargo.toml`:
```toml
agent = ["serde", "serde_json", "anyhow", "tokio", "chrono", 
         "async-trait", "umf", "cats", "regex", "wasmtime", 
         "wasmtime-wasi", "config", "observability", "checkpoint", 
         "provider", "orchestration", "executor"]
```

Enabling `agent` pulls in everything—this is the "God Object" anti-pattern.

---

**Document Status:** Complete analysis with actionable recommendations  
**Author:** Claude Opus 4.5 analysis
