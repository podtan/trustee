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

abk
abk[cli]
Files: x.rs, y.rs
File x.rs
Functions: a, b, c
Functions variables a = x
Function struct ....

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
