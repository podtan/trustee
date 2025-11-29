# Trustee Agent Architecture — GPT‑5.1 Notes

This document answers the questions from `AGENT_EVOLUTION_Part3.md`, based on the current code in `tmp/` (ABK, CATS, UMF, coder-lifecycle, tanbal-provider) and the coupling/data‑flow analyses.

---

## 1. Where should the shared types live?

### 1.1. Split types by responsibility, not by brand

The current problem is that ABK owns too many cross‑cutting types and imports UMF and CATS directly. To make modules replaceable, types should be split into three layers instead of one global "types" crate:

1. **Core Host ABI types (very small, very stable)**  
   - Ownership: a new crate, e.g. `trustee-abi` (or `trustee-interfaces`).  
   - Purpose: define *only* the minimal, provider‑ and tool‑agnostic interfaces used across crates:  
     - Provider side: traits and structs equivalent to `LlmProvider`, `GenerateConfig`, `GenerateResponse`, `ToolInvocation` – but expressed without directly embedding UMF types.  
     - Tool side: a trait for tool registries / tools ("call this tool name with JSON input → JSON output"), without depending on CATS.  
     - Lifecycle side: a small WIT‑like Rust trait surface mirroring what you already expose over WASM.  
   - These types should look like a C header / Kubernetes API object: versioned, backwards‑compatible, and intentionally boring.

2. **Domain types per crate**  
   - `umf` keeps all rich message types: `InternalMessage`, `MessageRole`, `ContentBlock`, `ToolCall`, etc.  
   - `cats` keeps tool definitions and internal helper types.  
   - `abk` keeps orchestration‑specific state types (e.g., `Checkpoint`, orchestration state machines), but *uses* ABI interfaces when talking to UMF/CATS, instead of importing their concrete types.

3. **Conversion boundaries**  
   - Define thin conversion modules at the edges (e.g., `abk::provider::umf_adapter`, `abk::tools::cats_adapter`) that translate between the stable ABI types and the rich domain types of UMF/CATS.

This mirrors how:
- **C ABI + headers**: headers define a small, stable C interface; each shared lib can evolve internally as long as the exported ABI matches.  
- **Kubernetes**: defines API objects (Pod, Deployment) in a central API; controllers and operators depend on these types but can be swapped because the API is stable.  
- **PostgreSQL**: exposes a small extension API (types/functions) in headers; extensions link against that without owning core types.

So: **don’t centralize *all* types into `trustee[types]` or `abk[types]`. Instead, create a very small `trustee-abi` crate for shared interfaces, keep domain types inside UMF/CATS, and use adapters.**

---

## 2. Non‑Rust native extensions (not via WASM)

You already have a clean pattern for polyglot integration via WASM + WIT; for native non‑Rust extensions there are two pragmatic options:

1. **C ABI plug‑ins (lowest common denominator)**  
   - Define a C‑compatible FFI surface (in `trustee-abi`): functions to create/destroy a provider/tool instance and call `generate`, `execute_tool`, etc.  
   - Other languages (C, C++, Zig, Rust, Nim, etc.) can implement this ABI and be loaded as shared libraries (`.so`) using `libloading` or similar.  
   - Pros: mature model, widely supported; mirrors how PostgreSQL extensions work.  
   - Cons: manual memory management, limited type system surface (you pass JSON / byte slices / simple structs).

2. **gRPC / HTTP micro‑extension pattern**  
   - Treat non‑Rust components as network services.  
   - Define a small protocol (HTTP+JSON, or gRPC/Protobuf) for provider/tool operations.  
   - ABK then has an `ExternalProvider` / `ExternalTool` that talks to these services.  
   - Pros: language‑agnostic, process‑isolated, easy to debug and scale; no ABI breakage across compiler / libc versions.  
   - Cons: adds network overhead; more ops complexity.

Given you already have WASM + WIT working well, a **pragmatic order** is:

- Keep **WASM** as the primary polyglot path.  
- For rare cases where you truly need native speed or tight OS integration, add a **C‑ABI plug‑in interface**, shaped like PostgreSQL’s extension API, but still using simple JSON/bytes payloads.

---

## 3. Low‑hanging architectural improvements

Based on the current tmp code and the coupling report, these are realistic, incremental fixes:

1. **Untangle `config` ↔ `cli`**  
   - Problem: `Configuration` contains an `Option<crate::cli::config::CliConfig>`, forcing `cli` to exist whenever `config` is used.  
   - Fix: move the CLI‑specific part out of `Configuration` into a separate `CliConfiguration` that is constructed *from* the generic `Configuration`.  
   - Result: `config` becomes usable with `provider`/`checkpoint` without dragging in CLI.

2. **Introduce a `ToolSystem` trait in ABK**  
   - Today ABK’s agent imports `cats::ToolRegistry` directly (`use cats::{create_tool_registry_with_open_window_size, ToolRegistry};`).  
   - Add an internal trait like `ToolSystem` ("get registry", "execute tool"), implemented in a small adapter crate `trustee-tools-cats` that wraps CATS.  
   - ABK depends only on `trustee-abi` + `trustee-tools-api`; the choice of using CATS lives in the binary (`trustee`) or in a small adapter crate.  
   - This follows Dependency Inversion: core depends on interfaces, adapters depend on concrete implementations.

3. **Narrow UMF usage inside ABK**  
   - Replace broad imports of UMF types with a smaller internal message interface ("text", "tool call", "tool result"), defined in `trustee-abi`.  
   - Provide an `umf_adapter` module that converts between ABI messages and UMF’s rich `InternalMessage`.  
   - That lets you keep UMF powerful while preserving the option to swap it, or to support multiple message formats.

4. **Give `lifecycle` its own feature flag**  
   - Right now `lifecycle` is bundled into `agent`. Introduce a `lifecycle` feature and make `agent` depend on it.  
   - That allows using lifecycle loading in other binaries or tests without the full agent stack.

5. **Hard limit: compile features independently in CI**  
   - Add CI jobs that compile each feature in isolation (`--no-default-features --features X`) to prevent regressions and force real modularity instead of phantom gates.

These changes are small, mechanical, and consistent with current Rust practice; they don’t require a big rewrite or new concepts.

---

## 4. What to do with `abk[agent]` and `abk[orchestration]`?

`abk[agent]` and `abk[orchestration]` exist because they were convenient extraction targets, but they now feel like God‑modules. A pragmatic cleanup path is:

1. **Demote `abk[agent]` to an example agent, not the only one**  
   - Keep `abk[agent]` for now, but treat it as a *reference implementation* built on top of the smaller ABI traits.  
   - Move the reusable pieces (session management, workflow steps, provider/tool adapters) behind internal traits/modules in ABK or `trustee-abi`.  
   - Over time, trim `Agent` until it mainly wires together: lifecycle, provider, tool system, checkpoint manager.

2. **Split `orchestration` along data‑flow boundaries**  
   - Identify clusters: conversation state management, decision loop ("call provider vs call tool"), checkpoint integration.  
   - Extract them into smaller modules (`conversation`, `decision_loop`, `checkpoint_bridge`) so you can test or reuse them separately.  
   - Keep the public surface of `orchestration` small (a `WorkflowCoordinator` facade) while shrinking the internals.

3. **Move policy decisions out of ABK into lifecycle / config**  
   - Things like "how many iterations", "when to call which tools", "what templates to use" belong naturally to lifecycle plugins and configuration, not to ABK itself.  
   - Over time, reduce the amount of hard‑coded policy in `Agent`/`orchestration`, letting lifecycles drive behavior via richer WASM interfaces.

Concretely: **don’t delete `abk[agent]` and `abk[orchestration]` now**; instead, make them thinner facades that rely on well‑defined interfaces. When they get sufficiently small and generic, you can decide whether to keep them in ABK or move a final version into the `trustee` binary.

---

## 5. Data‑flow as the core composition idea (post‑UDML)

UDML/URP pushed everything into runtime schemas and large message envelopes, which hurt type safety and performance. You can still keep "data flow first" as a design lens, but grounded in existing trends:

1. **Typed data‑flow inside the process**  
   - Keep Rust types as the authority for in‑process data.  
   - Represent each stage of the agent pipeline as:  
     - input type → function → output type  
     - e.g., `TaskDescription → ClassifiedTask`, `ClassifiedTask → ConversationState`, `ConversationState → ProviderRequest`, `ProviderResponse → UpdatedConversationState`.  
   - This looks like a data‑flow graph, but you stay in Rust’s type system instead of pushing everything into JSON.

2. **Schema‑backed messages only at boundaries**  
   - At boundaries (WASM, HTTP, C‑ABI plug‑ins), describe payloads with WIT / JSON Schema / Protobuf — but always generated from Rust types when possible.  
   - UMF already gives you a nice typed representation of messages; treat UDML/URP as an optional view over those types, not as the source of truth.

3. **Leverage existing patterns instead of a new DSL**  
   - For intra‑process data flow: use well‑structured modules and traits; if you want more formalism, a small typed pipeline framework ("step" traits, combinators) is enough.  
   - For inter‑process / plugin flow: WIT (for WASM) or Protobuf/OpenAPI (for HTTP/gRPC) are standard and well‑supported.

4. **Where UDML still fits**  
   - UDML can remain a *documentation / design* tool: a way to describe information, access, manipulation, extract, movement, coordination at a conceptual level.  
   - Implementation‑wise, keep Rust types + WIT/Protobuf as the concrete interfaces, and generate UDML views from code, not the other way around.

In short: **keep "data‑flow" as the mental model, but implement it with typed Rust stages and standard interface descriptions, instead of a universal runtime schema bus.**

---

## Summary

- Introduce a small `trustee-abi` crate for shared interfaces; keep rich types in UMF/CATS and connect them via adapters.  
- Prefer WASM for polyglot extensions; add a C‑ABI or HTTP/gRPC escape hatch only where needed.  
- Fix obvious coupling (config↔cli, ABK→CATS, ABK→UMF) with small refactors and CI feature checks.  
- Gradually thin `abk[agent]` and `abk[orchestration]` into facades built on clear interfaces, instead of deleting them outright.  
- Treat data‑flow as a typed pipeline plus explicit boundaries, not as a single UDML/URP message bus.
