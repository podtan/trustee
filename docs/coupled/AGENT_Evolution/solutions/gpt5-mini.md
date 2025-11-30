**Gpt5-Mini Solution Notes**

- **Purpose:** Answer the questions in `AGENT_EVOLUTION_Part3.md` using the coupled-docs, current data-flow analysis, and the temporary package sources under `tmp/`.
- **Scope:** Types placement, native-extension options, low-hanging fixes, ABK refactor suggestions, and data-flow guidance. Recommendations are pragmatic and incremental.

**Executive Summary:**
- **Problem:** The repository currently exhibits "phantom modularity": ABK exposes many feature-gated modules but still depends on UMF and CATS types at compile time, bundling lifecycle under `agent`, and leaking CLI types into core `config` types.
- **What works:** WASM-based plugins (`coder-lifecycle`, `tanbal-provider`) show a clean boundary using WIT/WASM and are good models to replicate.
- **Goal:** Make module boundaries real by (a) extracting minimal, stable type interfaces, (b) reversing ABK's direct dependency on CATS/UMF via small traits or type crates, and (c) enabling independent compilation for features.

**Answers to the Questions (numbered as in Part 3)**

1) Where to keep shared types: `trustee[types]` or `abk[types]`?
- Recommendation: introduce a small, dependency-free crate `trustee-types` (or `trustee_core_types`) that contains only the minimal, stable data types & traits used across runtimes: `InternalMessage` (thin), `MessageRole` (enum), `ToolCall` (id/name/input JSON), `ToolResult` (id/content/error), `Checkpoint` (session_id,timestamp,iteration,conversation minimal shape), and small trait interfaces such as `ToolRegistryTrait` and `LlmProviderTrait`.
- Rationale: putting these types in a top-level, micro crate avoids circular/table-stakes coupling inside `abk`. `abk` can depend on `trustee-types`; UMF/CATS may keep their richer implementations but expose adapters to the shared types. This mirrors the C ABI/header approach: a stable, tiny contract that rarely changes.
- Migration notes:
  - Start with `trustee-types` and migrate small slices of ABK to use the crate (types only).
  - Keep conversions/adapters in `umf` and `cats` crates: implement `From<umf::InternalMessage> for trustee_types::InternalMessage` and vice versa.
  - Use semver carefully: breakage in `trustee-types` should be rare and deliberate.

2) Native extensions in languages other than Rust vs WASM — pragmatic option?
- Recommendation: Keep WASM + WIT as the primary cross-language boundary for plugin semantics and portability. For native (non-WASM) integration, prefer a thin, well-defined FFI boundary or local RPC:
  - Option A (recommended): Continue using WASM for cross-language portability. WIT interfaces provide a safe, versioned contract as seen in `tmp/coder-lifecycle` and `tmp/tanbal-provider`.
  - Option B (native extension): If native performance and language ABI are required, provide a small, well-documented IPC/JSON-RPC interface (unix domain socket / local HTTP) and a `trustee-host` adapter in Rust that translates the `trustee-types` messages. This keeps the core safe and decoupled and avoids ABI compatibility pain across languages and OSes.
- Rationale: WASM enforces sandboxing, small payloads, and is already implemented in the repo. Native FFI is brittle and ties the build/test surfaces across languages and platforms.

3) Low-hanging fruits in the current architecture
- Add `trustee-types` (micro-crate) and conversion adapters in `umf` and `cats`.
- Make `lifecycle` a separate Cargo feature in `abk` instead of bundling it behind `agent`.
- Remove direct `cats` imports from `abk::agent` by depending on a trait (in `trustee-types` or `abk::interfaces`) and implementing a thin adapter that bridges `cats::ToolRegistry` to the trait.
- Refactor `abk::config::Configuration` to avoid including concrete `cli` types unconditionally. Instead:
  - Feature-gate the `cli` field: `#[cfg(feature = "cli")] pub cli: Option<cli::CliConfig>` or
  - Use an opaque `Value`/`serde_json::Value` for optional fields and supply typed helpers behind features.
- Add CI build matrix entries that compile `--features` individually (e.g., `executor`, `observability`, `provider`) to detect hidden compile-time coupling early.

4) How to remove or migrate `abk[agent]` and `abk[orchestration]` concerns
- Short plan:
  - Split responsibilities: move pure orchestration helpers that do not need UMF/CATS into `abk::orchestration_core` (small subset) and depend only on `trustee-types` and `executor`/`observability` features.
  - Keep `abk::agent` as a thin adapter that composes features; refactor it toward composition over direct type imports: `Agent` should accept `Box<dyn LlmProviderTrait>` and `Box<dyn ToolRegistryTrait>` so tests and alternate implementations can be swapped in.
  - Extract lifecycle binding code into `abk::lifecycle` as a first-class feature with minimal `trustee-types` dependencies; give it its own Cargo feature.

5) Data Flow, UDML, and workable approaches
- Use UDML/URP as a high-level orchestration layer or optional plugin, not as the hot-path runtime glue. Derive runtime schemas from Rust types when/if useful (codegen), not the other way around.
- Keep hot-paths small: pass small typed values between functions (traits + structs from `trustee-types`), and use URP/UDML only for large orchestration messages, dashboards, or cross-service coordination.

**Concrete Implementation Roadmap (incremental)**

1. Create `crates/trustee-types` (tiny, zero-deps except serde) with the canonical, minimal shared types.
   - Files: `src/lib.rs` with `InternalMessage`, `MessageRole`, `ToolCall`, `ToolResult`, `Checkpoint` and `traits.rs` with `LlmProviderTrait` and `ToolRegistryTrait`.
   - Cargo.toml: publish locally as version 0.1.0; add to workspace.

2. Add conversion adapters in `tmp/umf` and `tmp/cats`:
   - Implement `From`/`TryFrom` conversions between the crate-specific types and `trustee-types`.

3. Change `abk` to depend on `trustee-types` instead of `umf`/`cats` for its core `agent` interfaces.
   - Add new `abk` features: `lifecycle` (new standalone), `agent-core` (thin glue). Keep `agent` as composition that pulls features together.

4. Remove `cli` types from `abk::config::Configuration` public shape. Feature-guard the CLI field and add typed helpers behind the `cli` feature.

5. Add CI jobs compiling `cargo build -p abk --features executor` and other single features to ensure independent compilation.

6. Documentation updates: `docs/coupled/*` should reference `trustee-types` and give migration examples.

**Quick Commands / Dev Checklist**
```bash
# Add crate to workspace and build
cargo new crates/trustee-types --lib
# Add trustee-types to workspace Cargo.toml and implement minimal types
cargo build -p trustee-types

# Add conversions in tmp/umf and tmp/cats, then test
cd tmp/umf && cargo test
cd ../cats && cargo test

# Compile ABK with minimal features to confirm decoupling
cargo build -p abk --features "config,executor,observability"
```

**Evidence & Rationale from inspected sources**
- `tmp/tanbal-provider` and `tmp/coder-lifecycle` implement clean WASM WIT interfaces and have no ABK/UMF/CATS dependencies — these are good examples to replicate when designing cross-language boundaries.
- `tmp/abk/README.md` documents the feature-gated approach, but the `docs/coupled` evidence shows functions and types leak across modules; `trustee-types` will fix that leak by providing a tiny stable contract.
- `tmp/cats/AGENTS.md` describes a trait-based tool registry; creating a `ToolRegistryTrait` in `trustee-types` makes it possible for `abk` to depend only on the trait while `cats` implements it.

**Risks and mitigations**
- Risk: `trustee-types` becomes a dumping ground and grows too fast.
  - Mitigation: keep it deliberately minimal; only types that must be shared across crates live there. Everything else stays in crate-local implementations.
- Risk: Conversion code adds runtime cost.
  - Mitigation: conversions should be zero-cost where possible (From/TryFrom) and kept simple. Hot-path conversions stay in-process and are cheap.

**Next steps (if you want me to continue)**
- I can scaffold `crates/trustee-types` with the minimal types and add example `From` impls for `tmp/umf` and `tmp/cats`. Then update `abk`'s `Cargo.toml` to depend on it and run the single-feature compilation CI jobs locally.

---


