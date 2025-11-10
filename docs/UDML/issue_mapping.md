# ISSUE: Mapping Practical (Intuition + Brute-force) Breakup to UDML

## Summary

After building a ~25k-line Rust coding agent and then attempting to modularize it, the project now has two distinct framings of the same system: (1) a practical, pragmatic breakup that emerged from intuition and brute-force refactoring and is documented in `docs/coupled/*`, and (2) a conceptual data-centric model, the Unified Data Morphism Hypothesis (UDML), defined in `docs/UDML/UDML.md`.

This issue documents the problem: the practical module decomposition does not map cleanly to UDML's five data domains (Information, Access, Manipulation, Extract, Movement). The intent of this file is to capture the story, the concrete mismatches, and the open questions that block a faithful, traceable mapping — not to propose solutions.

## The story (how we got here)

1. The project began as a single monolithic Rust codebase (~25k LOC) implementing a coding agent. Functionality was added organically: CLI, runtime agent, provider adapters, lifecycle plugins, tool registry, checkpointing, observability, etc.

2. To make the codebase maintainable, I (the author) started to break it up using domain knowledge and pragmatic judgment. Some splits were deliberate (move lifecycle into WASM plugins); others were brute-force (split large files by obvious responsibility boundaries). This produced the "practical" layout and supporting documentation in `docs/coupled/tightly_coupled_hypothesis.md` and `docs/coupled/data_flow_diagram.md`.

3. Later, I designed a formal model (UDML) — a data-first philosophy that describes every software system in terms of five data domains: Information, Access, Manipulation, Extract, and Movement. UDML aims to simplify modularity: new ownership if ownership is needed, new module for new movement rules.

4. Now I face a translation problem: I cannot confidently map many of the practical modules to UDML. I can see some obvious correspondences (UMF → Information + Movement), but in many cases the mapping is ambiguous or conflicting with the original intuition-driven splits.

## Concrete items involved (practical modules)

- `abk::agent` (Agent runtime / wiring)
- `abk::cli` (CLI/bootstrap)
- `abk[config]` (Configuration loader)
- `abk[provider]` / `ProviderFactory` (LLM provider factory + trait)
- `abk[lifecycle]` (WASM lifecycle templates & classification)
- `abk[executor]` (Command executor)
- `abk[checkpoint]` (Checkpoint/session manager)
- `abk[orchestration]` (Workflow coordinator)
- `abk[observability]` (Logging/telemetry)
- `CATS` (Tool registry + tool implementations)
- `UMF` (InternalMessage, ContentBlock, ToolCall/ToolResult, ChatML helpers)
- `Provider-WASM / Tanbal` (WASM provider)
- `Lifecycle-WASM` (WASM lifecycle templates)

## Types of mapping problems observed

1. Feature/Build-vs-Ownership mismatch
   - Much of the code is grouped by `abk` feature flags rather than by independent ownership. This creates modules that exist as compile-time knobs rather than runtime-owned components. Under UDML, ownership is a first-class decision; the build-driven grouping obscures it.

2. Movement boundary ambiguity
   - UDML elevates "movement" (how data crosses boundaries) as a primary axis for modularity. In the practical breakup, movement is often implicit: providers, tools and lifecycles are sometimes compiled in, sometimes loaded as WASM, and the exact movement semantics are not consistently documented.

3. Information shape fragmentation
   - Canonical data shapes (UMF message types, checkpoint schema, config shapes) are used across components but lack a single canonical owner. This fragments the "Information" domain and makes it hard to declare who owns schema evolution.

4. Cross-cutting responsibilities
   - Observability, orchestration and configuration are cross-cutting in practice. Under UDML they map to Extract/Movement/Access concerns that demand clearer ownership and narrower contracts, but the pragmatic layout keeps them mixed into `abk`.

5. Implicit contracts vs explicit rules
   - The practical code contains many implicit expectations (message formats, template names, provider behavior). UDML asks for explicit movement and extract rules; their absence makes the mapping speculative.

## Why this is a problem (risks & blockers)

- Without a clear mapping, refactors that aim to adopt UDML risk breaking runtime behavior or losing the original reasoning behind the pragmatic splits.
- It is difficult to write migration tests or compatibility shims if Ownership and Movement are not explicitly resolved.
- Decision-making stalls: which components become independent crates/plugins, and which stay as internal modules?

## Open questions (to resolve before proposing a refactor)

- Which data shapes (UMF, checkpoint, config) are canonical and who owns them?
- Which boundaries are true "movement" boundaries (require plugins, network, or runtime isolation) and which are implementation details?
- Are `abk` feature-flag groupings intended as long-term ownership, or were they a short-term convenience?
- Where are the explicit contracts for WASM lifecycles and WASM providers, and are they stable enough to be used as movement interfaces?


---

This file captures the issue description and the narrative behind the mapping difficulty. It intentionally omits proposed fixes: those will be separate, actionable artifacts once the open questions above are answered and the canonical Information and Movement rules are declared.