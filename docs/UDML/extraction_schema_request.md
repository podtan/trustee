## Request: Design SQLite schema and extraction for repository-wide code & config metadata

### Summary

We need a clear, scoped task to design a canonical SQLite schema and extraction process that captures repository-wide metadata for every project and file. The goal is to be able to query code and configuration artifacts programmatically (functions, structs, variables, config keys, file metadata, comments, etc.) and power downstream tools and analyses.

This document is an issue/request only — it intentionally does NOT contain a proposed schema or implementation details. It describes what to capture, constraints, acceptance criteria, and next steps for the team to design the schema and implementation.

### Background / Motivation

Right now we want to extract structural metadata across this workspace (all crates, modules, files, and config files). The preferred storage is an SQLite database to enable fast queries, portability, and ad-hoc analysis. The first task is to design a schema that covers code artifacts (Rust and other languages present), config files (TOML, YAML, ENV, JSON), and provenance information (file path, repo, commit/sha if available).

The attached documentation files in the repository contain additional context and diagrams that should be considered when designing the schema and extraction strategy. See repository docs for details.

### Goal

Collect, normalize, and persist the following kinds of metadata into a single SQLite database for the entire repository (per project/crate and per file):

- Project/Crate metadata: name, path, manifest files (e.g., `Cargo.toml`), root module(s), language(s), and optional VCS metadata (commit/sha when available).
- File metadata: repository-relative path, language, size, last-modified timestamp, checksum (e.g., sha256), and file role (source, test, example, config, build script, documentation).
- Code symbols: functions, methods, free functions, constants, statics, variables, structs, enums, traits, type aliases, macros, module definitions, and their signatures/visibility.
- Symbol details: name, fully-qualified path (module path), signature or fields (for structs/enums), return type, parameters (name + type + default when available), visibility (pub/private), attributes, doc comments, and any inline annotations (TODO/FIXME tags).
- Relationships: file → defines → symbol, symbol → type → other symbols (references), impls and trait implementations, module nesting and re-exports.
- Imports/uses: top-level `use`/import statements (source and resolved if possible), and external dependency references (crate names and versions where applicable from manifests).
- Config keys and values: parsed key/value pairs for common config formats (TOML, YAML, JSON, env files). Store both raw representation and typed value where applicable, with provenance to file and location.
- Comments and docstrings: capture doc comments (summary and full doc) attached to symbols and files, and optionally index TODO/FIXME notes with location.
- Metadata and provenance: source file path, line/column ranges for symbol definitions, parser version, extraction timestamp, and optional git commit sha or tag when extraction runs in CI.

### Example (human-readable) output request

The consumer expects to be able to produce per-project, per-file summaries like the example below when needed. This is NOT a schema — it is an example of the desired extracted information.

Project: abk
Path: /crates/abk
Manifest: /crates/abk/Cargo.toml
Languages: Rust

Files (summary):
- src/lib.rs (source) — 12.3 KB, 2025-09-01T12:34:10Z, sha256:9f2...a7c
- src/cli.rs (source) — 4.8 KB, 2025-09-01T12:34:11Z, sha256:3b1...9d2
- tests/integration.rs (test) — 3.1 KB, 2025-09-02T08:01:22Z, sha256:efe...11b
- Cargo.toml (manifest/config) — 1.6 KB, 2025-08-31T18:20:00Z, sha256:aa4...c0f

File: src/cli.rs
- Role: source (binary/CLI)
- Path: crates/abk/src/cli.rs
- Size: 4,812 bytes
- Last modified: 2025-09-01T12:34:11Z
- Checksum: sha256:3b1...9d2
- Parser: rustc-syn 2.0.1
- Defined symbols (counts): functions=4, structs=2, enums=1, traits=0

Symbols (detailed):
- fn build_cli() -> clap::Command
   - Visibility: pub(crate)
   - Defined: src/cli.rs:12:1-28:1
   - Doc: "Create CLI command structure for the agent"
   - Attributes: #[inline]

- pub struct CliArgs
   - Fields:
      - verbose: bool (line 31)
      - config: Option<PathBuf> (line 32)
   - Visibility: pub
   - Defined: src/cli.rs:30:1-42:1
   - Doc: "Parsed CLI arguments"

- fn run_command(args: &CliArgs) -> Result<(), Error>
   - Visibility: pub
   - Signature: (&CliArgs) -> Result<(), Error>
   - Defined: src/cli.rs:50:1-98:1
   - Doc: "Execute the main CLI flow; handles subcommands 'run' and 'test'"
   - Inline TODOs: `// TODO: support --parallel` at src/cli.rs:72:5

Imports / uses (top-level):
- use clap::{Command, Arg};
- use crate::executor::Executor; // local crate reference

Relationships / references (human-readable):
- `CliArgs` is instantiated in `main.rs` and passed to `run_command` (call sites: src/main.rs:14)
- `run_command` calls `Executor::spawn` (resolved to crate `abk::executor::Executor::spawn`)
- `build_cli` is re-exported by `lib.rs` as `pub use cli::build_cli;`

Config keys (example from Cargo.toml):
- package.name = "abk" (file: Cargo.toml:2:10)
- package.version = "0.1.24" (file: Cargo.toml:3:10)
- dependencies.clap = { version = "4.0", features = ["derive"] } (file: Cargo.toml:12:1)

Comments / docs / notes found:
- File-level doc summary (src/lib.rs): "ABK - Agent Builder Kit, provides modular agent components."
- Doc for `build_cli`: "Create CLI command structure for the agent"
- TODO / FIXME index:
   - TODO: support --parallel (src/cli.rs:72)
   - FIXME: handle edge-case for empty stdin in Executor::spawn (src/executor/mod.rs:128)

Per-repo summary (example output of an extractor run):
- Projects: 5
- Files indexed: 312
- Total symbols: 1,428 (functions: 924, structs: 210, enums: 74, traits: 22)
- Config entries: 48
- Extraction timestamp: 2025-11-09T10:02:12Z
- Extractor version: extractor/0.2.0
- VCS commit (if available): git sha: 7a9c5f2e

Example: query result — functions named `build_cli`

- build_cli
   - Project: abk
   - File: crates/abk/src/cli.rs
   - Signature: fn build_cli() -> clap::Command
   - Visibility: pub(crate)
   - Defined at: crates/abk/src/cli.rs:12:1-28:1
   - Doc summary: "Create CLI command structure for the agent"

Example: config key provenance

- dependencies.clap -> found in crates/abk/Cargo.toml at line 12, column 1; value: { version = "4.0", features = ["derive"] }

Notes:
- These human-readable summaries are examples of how the extracted rows should be presented; the underlying SQLite schema should support producing these views via queries (counts, joins, and text aggregation). The extractor should capture raw text and location metadata so that these summaries can include signatures, doc excerpts, and TODO/FIXME annotations.

The SQLite schema should enable these textual summaries as well as structured queries.

### Constraints and non-functional requirements

- Storage: single-file SQLite DB; support for incremental updates (upserts) is preferred.
- Extensibility: the schema should be language-agnostic but expressive for Rust (and other languages present). Parsers should be pluggable.
- Provenance & reproducibility: store extraction timestamp, tool version, and (if available) the VCS commit/sha used for extraction.
- Performance: extraction should scale to a medium-sized repo (tens of thousands of lines) and be reasonably fast. Schema should allow indexes on common query keys (project, file, symbol name, symbol kind).
- Idempotence: repeated runs on the same inputs should not create duplicates; the extractor should be able to re-run and update rows as source changes.
- Security/privacy: do not leak secrets in configuration files; extractor should label or redact sensitive values (or provide an opt-out) as part of implementation planning.

### Acceptance criteria (what 'done' looks like for the schema design task)

1. A documented SQLite schema (separate deliverable) that maps the items listed in "Goal" to tables/columns and indexes. The schema document must include rationale for chosen normalization, primary keys, and foreign keys.
2. Example SQL queries that demonstrate retrieving common views:
   - list all files for a project with counts of functions/structs
   - fetch all functions named `foo` and their defining file + signature
   - query all config keys from `Cargo.toml` and their values with file provenance
3. A short migration strategy describing how to convert earlier extract formats (if any) and how to perform incremental updates.
4. A small checklist for the implementation step: recommended parsers, test corpus, and sample extraction run on a small subset of the repo.

### Acceptance constraints (what the request forbids in this issue)

- Do not include the actual schema or SQL DDL in this issue file. This file only describes the request and acceptance criteria for designing the schema.

### Next steps (for the implementer assigned)

1. Design the SQLite schema (produce a separate design document or PR containing the schema DDL and explanations).
2. Propose an extractor implementation plan (language, libraries, parsing approach) and test harness.
3. Implement a minimal proof-of-concept extractor that runs on this repository and populates the DB for a subset (e.g., `src/` and top-level `Cargo.toml`).
4. Add tests and example queries to validate results.

### Notes / References

- Attached context files (see top of this issue) contain domain notes and diagrams helpful for scoping and prioritization.
- When in doubt, prioritize accurate provenance and conservative parsing (i.e., capture raw text and location in addition to any parsed/type-inferred values).

---

This file is intentionally an ISSUE description only. Do not add schema, SQL, or implementation details here — those belong in the schema design deliverable that follows this request.
