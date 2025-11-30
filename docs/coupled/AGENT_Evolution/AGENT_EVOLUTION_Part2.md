## Agent Evolution — Part 2

This file continues the story in `AGENT_EVOLUTION_Part1.md` from the perspective of a CTO who trusts her leads but follows the old Russian proverb: "trust, but verify." It records two experiments I ran to make Trustee more extensible and understandable, why they failed, and a pragmatic path forward.

### CTO framing: trust, but verify

As CTO I usually trust Managers, Team Leads and Experts (Developers / DevOps / DBA / ...). I use the approach of the Russian proverb: I trust, but verify. In practice that means I accept creative proposals from my teams (and from LLMs), but I make sure there are observable boundaries, tests, and instrumentation so we can validate assumptions and recover when a decision turns out to be wrong.

In developing Trustee I use LLMs for "vibe coding" and even to help shape architecture ideas. That was productive, but it also surfaced two important risks: (1) design drift (ideas that feel appealing but are fragile in practice), and (2) false modularity (a directory or feature flag that looks like a module but is tightly coupled at build/time).

My goals for Trustee were simple and sequential:

1. Create working code.
2. Break it into a modular system.
3. Make it extensible.

I tried two different approaches to get there. Both provided insight, but both failed in ways that taught me important lessons.

---

### First Attempt — automatic input/output extraction

Idea: automatically extract inputs and outputs of every module and analyze the code so we and LLMs could use that metadata to continue modular development.

Why I tried it: if we could auto-generate a data-flow map and I/O contracts, we could (a) discover implicit dependencies, (b) automatically generate adapters and tests, and (c) feed the results back to LLMs to guide refactors and extractors.

Why it failed:

- Scope complexity: a reliable extractor that fully understands data flow across a real codebase would need language-specific parsers and deep semantic analysis. Building and maintaining such a tool is almost as complex as parts of the codebase itself.
- Multi-language surface: Trustee uses Rust plus WASM-boundary components and potentially other languages. Building accurate cross-language extraction is expensive.
- Tooling mismatch: existing tools like SonarQube or CodeQL are powerful but heavyweight and not designed to produce the compact, consumable data-flow artifacts I wanted. They also produce noise and are difficult to tailor for the small, high-precision signal (I/O pairs) I needed.

Conclusion: full automatic extraction was a dead end as a first step — it became a project unto itself.

---

### Second Attempt — black-box modules + UDML/URP

UDML (which I designed) reframes everything as data: information, access, manipulation, extract, movement, and coordination. Key UDML ideas (brief):

- Look at everything as data — design-time specs drive runtime behavior via URP packets.
- SQL comparison: Information=DDL, Access=DCL, Manipulation=DML, Extract=DQL; Movement and Coordination are first-class domains.
- Treat module boundaries as data schemas (derive schemas from code, not replace types with schemas).
- Modular rule set: Two questions → Two rules:
	1. Does this need different ownership? → create a new module.
	2. Does this need new movement/coordination? → create a new movement/coordination rule and evaluate splitting the module.

Why it failed (three main reasons):

1. Type-safety loss: moving authoritative types from compile-time (Rust types) into runtime schemas (YAML/JSON) forces us to shift type checks from the compiler to runtime. That loses the benefits of static typing and was unacceptable for core modules where safety and correctness matter.
2. Performance and verbosity: passing large URP documents rather than small values increased overhead and amplified serialization/deserialization costs. For performance-critical paths this was too costly.
3. Reinventing an ESB: when I framed everything as messages and schemas I realized I was effectively building an enterprise service bus (ESB) with different vocabulary. That model may be valid for some domains, but Trustee didn't need a full ESB — it needed clear, small boundaries and stable ABI-like interfaces.

I also briefly considered a hybrid approach: authoring JSON/YAML schemas and using a Rust macro/codegen step to generate Rust types at compile time. That felt attractive because it would keep types in Rust while still making schemas explicit, but in practice it didn't seem clean or simple. Macro-driven codegen couples build tooling to schema files, introduces non-obvious compile-time complexity, and pushes validation into macros and build scripts rather than the language's natural type system. For these reasons I rejected it as the primary path.

Conclusion: UDML/URP is useful as an experiment and for higher-level orchestration, but treating it as the primary runtime glue for all modules broke important properties (type safety, performance, simplicity).

---

### Lessons learned

- Keep the compiler in the loop: preserve static types for the code that needs them. Use runtime schemas, but derive them from static types when possible (codegen), not the other way around.
- Boundaries matter: the right abstraction is a thin, language-stable boundary (traits, interfaces, or WIT for WASM) rather than a large runtime schema passed everywhere.
- Use existing, battle-tested boundaries where possible: the WASM + WIT approach in providers/lifecycles proved clean and decoupled; replicate that pattern for other extensibility points.
- Instrument first: before trying heavy refactors, add logging, telemetry, and cheap runtime validators so you can "verify" what is actually flowing through the system.
- Prefer small messages: avoid monolithic URP payloads inside hot paths. Use small, explicit arguments, and aggregate only when helpful for higher-level orchestration.
- Incremental wins: large, perfect solutions (perfect code extractor, perfect IDL) are expensive. Incremental steps that keep the system working are more valuable.

---
