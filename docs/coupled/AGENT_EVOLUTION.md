# The Evolution of the Trustee Agent

This is the story of how a personal coding experiment, `simpaticoder`, grew into the modular, lifecycle-first agent now known as Trustee. It preserves the original intent, design decisions, and lessons learned along the way.

## Background — the original goal

I wanted to create a coding AGENT whose behavior would be spec-driven: supply a specification, and the agent would write, test, and iterate on code. The early project was called `simpaticoder` and the immediate motivation behind many technical choices was practical productivity.

Why Rust?

- I was testing "vibe coding": if I only write specs, the productivity differences between languages shrink. Still, I wanted a language that was both fast and safe — faster than Python/Go and with stronger safety guarantees. Rust fit that requirement.
- Choosing Rust also enabled interop options. By using `pyo3` I could expose functionality to Python and reuse the rich Python ecosystem when needed.

## A simple definition of an AGENT

Over time my working definition of an agent became intentionally simple and standard: "an LLM calls tools inside a loop." I liked that simplicity and believed a useful coding agent could be compact — maybe even achievable with ten lines of code in the main app.

But the prototype grew. `simpaticoder` became a monolithic working application around 25,000 lines of Rust. It worked, but my long-term goal was a spec-driven coding agent that was modular and extensible.

## Design goals: minimal core, maximum extensibility

I wanted a small, simple core and the rest extensible — like Kubernetes or PostgreSQL: powerful, pluggable, and swappable. I thought about five complementary paths to extensibility:

1. Modules that don't require extreme speed could be written in any language, compiled to WASM, and plugged in.
2. Modules that need performance could be written as Rust crates.
3. Expose agent functionality to Python via `pyo3` to tap into Python tooling and ecosystems.
4. Emulate PostgreSQL's extension model: lots of small, discoverable extensions.
5. Learn from Kubernetes: provide clear extension points so components (written in many languages) can interoperate.

## Early modularization: CATS and WASM

The first clear separation was to pull tool definitions out of the agent. Coding Agent ToolS (CATS) was created as a separate crate and published to crates.io. The idea was that tools should be ordinary libraries that the agent can invoke, test, and evolve independently.

Because many LLM providers did not have official Rust SDKs (they tended to publish Python, TypeScript, Go, C#, Java bindings), I built a WASM provider architecture so providers could be implemented in whichever language made sense and exposed to the Rust agent through a WASM boundary.

I also realized the agent itself could morph into different roles. A lifecycle system implemented as WASM plugins would allow the agent to behave differently (coding agent, customer-support agent, test agent, etc.) without changing the core runtime.

## Message format and UMF

Different providers expect different message formats. To avoid coupling to any single provider, I extracted an internal message format and formatter that could be translated to OpenAI, Anthropic, Google, or other provider formats. This became the `UMF` crate (Universal Message Format).

## ABK: Agent Builder Kit

As separate crates proliferated, I noticed many cross-cutting utilities kept being reimplemented. To make reuse easy and offer a feature-gated surface, I created `ABK` (Agent Builder Kit). `ABK` collects utilities behind Cargo features such as:

- `config`
- `observability`
- `checkpoint`
- `provider`
- `executor`
- `orchestration`
- `cli`
- `agent`
- `lifecycle`

This allowed projects to depend on the building blocks they needed without pulling the whole monolith.

## Extraction and the 10-line rule: a saga with LLMs

I set a strict rule to reduce the main `simpaticoder` binary to a tiny bootstrap:

1. The main app (`simpaticoder`) must have at most ten lines of Rust code.
2. Unlimited configuration files stay in the main app.
3. All nontrivial Rust code must live in crates such as `ABK`, `CATS`, `UMF`, or provider/lifecycle crates.

Extraction was straightforward at first: `abk[config]`, `abk[observability]`, `abk[provider]`, `abk[lifecycle]`, and `abk[executor]` were successfully moved. Then I hit the wall: I had around 6,500 lines of code in `simpaticoder` and my target was to reduce the main binary to 10 lines. The biggest remaining part was the CLI—about 4,000 lines of Rust code—tangled together with orchestration and session logic.


LLMs were unable to extract the CLI automatically. I tried many models (GPT-5, Claude Sonnet 4.5, Grok-code-fast-1, Gemini 2.5), and none solved the extraction cleanly. I even struggled for two days with Claude Sonnet 4.5 trying to get a clean extraction of `abk[cli]` — it could not extract the CLI according to my rule. I started brute-forcing different prompts and approaches. Finally I made a strict rule:

1. Only 10 lines of Rust code in the main `simpaticoder` binary.
2. Unlimited configuration files in the main app.
3. Unlimited Rust code allowed in crates such as `ABK`, `CATS`, `UMF`, and provider/lifecycle crates.

Claude Sonnet 4.5 told me to extract the CLI into `abk[cli]` and to extract orchestration as `abk[orchestration]`. I was not happy with this because 1,500 lines for orchestration felt too large and, in my view, should have been handled in `main` or other clearer modules. Still, I accepted the change and the orchestration code was abstracted into `abk[orchestration]`.

After that, Claude was still unable to extract `abk[cli]` cleanly and recommended creating another module, `abk[agent]`. That was unacceptable at first—`simpaticoder` was supposed to be the agent—but I accepted the suggestion because my priority was to remove the 4,000-line CLI from the main app: the CLI logic was not the agent's core. Claude Sonnet 4.5 created `abk[agent]`, but even after that it could not extract `abk[cli]` according to my 10-line rule.

Grok-code-fast-1 eventually succeeded where others struggled by choosing a minimal interface pattern. Rather than inventing heavyweight behavioral factories, Grok used a small data-query interface (a `CommandContext`-style trait) as the thin bridge between the tiny bootstrap and the heavy CLI/orchestration code. The practical difference was:

- Claude's mistake: introduced an `AgentFactory` trait as an intermediate behavioral abstraction, expanding the surface area and blurring responsibilities.
- Grok's solution: used a `CommandContext`-style minimal query interface that hid implementation details and allowed safe extraction.

Importantly, the comparison above and the explanation for why Grok succeeded (and why Claude failed) come from the LLMs' analyses — I used GPT-5, Claude, and Grok-code-fast-1 to investigate the failure modes and the successful approach. Their summary was:

Grok succeed because:
- Used data-query interface instead of behavior-command interface
- Query pattern vs Command Pattern

I do not claim these phrases are authoritative for Rust; I even do not know if these analogies directly apply to Rust programming.

This approach favored a query-oriented interface (thin, explicit data queries) over an expansive command/behavior interface and made it possible to keep the agent bootstrap tiny while moving the heavy logic into feature crates.

## Concessions and modular boundaries

During the extraction I had to accept some uncomfortable but pragmatic boundaries:

- A large `abk[orchestration]` module (roughly 1,500 lines) was extracted — I had hoped orchestration would stay smaller or in the `main`, but the separation made testing and reuse much easier.
- `abk[cli]` eventually needed to become its own feature-crate; the CLI's complexity justified its separation from the tiny agent core.
- `abk[agent]` was created to gather higher-level agent orchestration glue, even though the original intention was to keep `simpaticoder` as the single agent entry. Pragmatism won: extract, test, and reuse.

At one point Claude deleted code and reintroduced larger chunks into the main app; Grok's approach preserved the extraction rule. After these iterations, the rule stood:

- Keep `simpaticoder` main minimal (10 lines). Put all substantial logic in feature crates.

## Final architecture: Trustee

The final result is a lifecycle-first, modular agent framework with a tiny bootstrap and many composable pieces:

- `CATS` — Coding Agent ToolS (tool definitions and helpers)
- `UMF` — Universal Message Format (provider-agnostic message representation)
- `ABK` — Agent Builder Kit (feature-gated utilities and orchestration)
- `providers/` — WASM-based provider implementations (e.g., `providers/tanbal`)
- `lifecycle/` — WASM lifecycle plugins to morph agent behavior

The runtime is small and simple; behavior and integrations live in plugins and feature crates. Providers and lifecycles run as WASM plugins when appropriate, and performance-critical pieces remain native Rust crates.

## Lessons learned

- Simplicity wins: define a minimal agent contract and keep the bootstrap tiny.
- Modularity enables reuse: separate tools, messaging, orchestration, and provider code into independent crates.
- WASM is powerful for polyglot integrations: it lets providers and lifecycles be implemented in other languages while keeping a secure boundary.
- Small interfaces beat large abstractions: a minimal query interface (`CommandContext`-style) is often more robust and easier to extract than heavyweight behavioral factories.
- LLMs help but don’t replace careful design: automated refactoring with LLMs can accelerate work, but human judgment is necessary to choose appropriate abstractions.

## Current state and next steps

The project that started as `simpaticoder` is now Trustee: a composable, lifecycle-first agent platform. Its structure makes it possible to:

- Morph an agent into many roles by swapping lifecycle WASM plugins.
- Add new providers by dropping WASM provider modules into `providers/`.
- Reuse the same core building blocks (`ABK`, `CATS`, `UMF`) across projects.
