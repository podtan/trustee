# UDML Schema Standardization (GPT5-Codex)

## Variation Highlights
- GPT5-codex outputs include explicit `format`, `component`, `summary`, and structured `context`, while claude-sonnet-4.5 relies on comment headers and JSON Schema fragments, and gemini-2.5-pro wraps the payload under a `udml` block with minimal metadata.
- Domain entries fluctuate between rich objects (ids, ownership, invariants) and bare descriptive strings; invariants and interfaces appear in GPT5-codex but are omitted or renamed elsewhere.
- Naming conventions differ: underscores in GPT5-codex filenames and ids, hyphenated keys in claude variants, camelCase and enum literals in gemini outputs, creating friction for automated tooling.
- Treatment of schema references varies (JSON Schema `$ref`, prose-only descriptions, or free-form strings), leading to inconsistent validation and discoverability of related artifacts.
- Coverage of the six UDML domains is uneven: some files omit `coordination` primitives or conflate `movement` with `manipulation`, making lifecycle comparisons difficult.

## Schema Overview
- **format**: fixed string `UDML-YAML` to signal the canonical document type.
- **schema_version**: semantic version of this specification (start with `0.2.0`).
- **component**: structured metadata about the subject (id, name, summary, capability tags).
- **metadata**: authorship, provenance, runtime version, review timestamps.
- **context**: wiring information pulled from the hosting crate (features, upstream/downstream interfaces, external dependencies).
- **domains**: map containing the six UDML domains; each domain hosts an ordered list of entries using domain-specific fields.

```yaml
format: UDML-YAML
schema_version: "0.2.0"
component:
  id: abk::agent
  name: ABK Agent Runtime
  summary: Runtime core wiring lifecycle, provider, tools, and checkpoints.
  kind: crate_module        # enum: crate, crate_module, wasm_plugin, tool, service
  tags: [agent, orchestration, runtime]
metadata:
  owners: ["Trustee Core"]
  source_repo: "https://github.com/podtan/trustee"
  source_ref: "<commit-sha>"
  reviewed_at: "2025-11-10"
context:
  crate: abk
  features: [agent, orchestration, lifecycle]
  upstream_interfaces:
    - umf::InternalMessage
    - cats::ToolRegistry
  downstream_interfaces:
    - abk::provider::LlmProvider
    - abk::checkpoint::CheckpointStore
domains:
  information: []
  access: []
  manipulation: []
  extract: []
  movement: []
  coordination: []
```

## Domain Specifications

### Information Domain
- **Required fields**: `id`, `ownership`, `summary`, `schema`.
- **Optional fields**: `lifecycle` (`transient|persistent|derived`), `constraints`, `retention`, `sensitivity`, `notes`.
- **Schema expectations**: prefer JSON Schema fragments; allow inline Rust type signatures under `schema.rust`.
- **Example entry**:
  ```yaml
  - id: session_state
    ownership: abk::agent
    summary: Aggregated runtime state for active session lifecycle.
    schema:
      json: |
        type: object
        required: [session_id, lifecycle, messages]
        properties:
          session_id: { type: string }
          lifecycle: { $ref: "lifecycle.udml.yaml#/information/lifecycle_context" }
          messages: { type: array, items: { $ref: "umf.udml.yaml#/information/internal_message" } }
    lifecycle: transient
    constraints:
      - tool_results must reference existing tool_call_id
  ```

### Access Domain
- **Required fields**: `id`, `actors`, `mode`, `scope`, `summary`.
- **Optional fields**: `authorization`, `auditing`, `notes`.
- **Mode enumeration**: `read`, `write`, `read_write`, `admin`.
- **Guideline**: detail how orchestration, operators, and external services can touch the information structures.

```yaml
- id: checkpoint_restore
  actors: [abk::agent, abk::checkpoint]
  mode: read_write
  scope: session_state
  summary: Agent may request and persist session snapshots via checkpoint store.
  authorization:
    required_roles: [orchestration-loop]
    escalation: "manual approval for cross-session restore"
  auditing: log_to: abk::observability
```

### Manipulation Domain
- **Required fields**: `id`, `summary`, `operations`.
- **Optional fields**: `preconditions`, `postconditions`, `failure_modes`.
- **Operations**: list of verbs with argument schemas (`name`, `inputs`, `result`).

```yaml
- id: message_mutations
  summary: Append user, assistant, and tool messages under UMF invariants.
  operations:
    - name: append_user
      inputs: { message: umf::InternalMessage }
      result: { type: void }
    - name: attach_tool_result
      inputs: { result: umf::ToolResult }
      result: { type: void }
  preconditions:
    - message.role in ["user", "assistant", "tool"]
  postconditions:
    - session_state.messages[-1] == message
```

### Extract Domain
- **Required fields**: `id`, `summary`, `outputs`.
- **Optional fields**: `inputs`, `algorithm`, `notes`.
- Document how new representations (summaries, projections, analytics) are generated.

```yaml
- id: chatml_projection
  summary: Transform UMF messages into provider-ready ChatML payload.
  inputs: [session_state.messages]
  outputs:
    - name: chatml_transcript
      schema: { type: string, format: text }
    - name: tool_envelopes
      schema: { type: array, items: { $ref: "umf.udml.yaml#/information/tool_call" } }
  algorithm: umf::chatml::render_transcript
```

### Movement Domain
- **Required fields**: `id`, `summary`, `source`, `destination`, `payload`.
- **Optional fields**: `protocol`, `frequency`, `boundaries`, `notes`.
- Capture every hop across processes, WASM boundaries, or persistence layers.

```yaml
- id: provider_request_dispatch
  summary: Send ChatML transcript to configured LLM provider.
  source: abk::agent
  destination: abk::provider::LlmProvider
  payload:
    schema:
      type: object
      properties:
        transcript: { type: string }
        config: { $ref: "abk_provider.udml.yaml#/information/generate_config" }
  protocol: rust_trait_call > wasm > https
  boundaries: [runtime, wasm, network]
```

### Coordination Domain
- **Required fields**: `id`, `summary`, `participants`, `primitives`.
- **Optional fields**: `triggers`, `schedules`, `failure_modes`, `notes`.
- `primitives` describe synchronization constructs (sequence, fanout, retry, consensus).

```yaml
- id: orchestration_cycle
  summary: Turn-based control loop coordinating provider calls and tool executions.
  participants: [abk::agent, abk::orchestration, cats::ToolRegistry, abk::provider]
  primitives:
    - type: sequence
      description: "process inbound message -> choose action -> emit request"
    - type: conditional
      description: "branch on provider response status"
    - type: retry
      description: "exponential backoff on provider transport errors"
  triggers:
    - message_received
    - checkpoint_interval
```

## Cross-Cutting Conventions
- **Identifiers**: use snake_case for ids and keys; namespace component ids with Rust path (e.g., `abk::agent`).
- **Ordering**: maintain domain order `information → access → manipulation → extract → movement → coordination`.
- **Schema fragments**: prefer JSON Schema draft 2020-12; embed under `schema.json`. Alternate language-specific definitions belong under `schema.rust` or `schema.sql`.
- **References**: use relative file references with fragment identifiers (`umf.udml.yaml#/information/internal_message`).
- **Optionality**: explicitly mark optional collections as empty arrays instead of omitting domains to keep the shape stable.
- **Comments**: avoid leading `#` comment banners so parsers can treat the document as pure YAML.

## Decision Rationale
- **Unified metadata envelope**: GPT5-codex documents already expose `format` and `context`; extending to `schema_version`, `metadata`, and `kind` keeps machine-readability while satisfying claude’s desire for descriptive headers.
- **Domain entry structure**: claude variants demonstrate rich JSON Schema, while gemini opts for prose-only bullets. Requiring `id`, `summary`, and domain-specific fields preserves expressiveness and eliminates ambiguous strings.
- **JSON Schema preference**: multiple models attempted schema fragments; normalizing them under `schema.json` prevents intermixing of prose and code, enabling validation pipelines.
- **Enumerated primitives**: naming the allowed `mode`, `lifecycle`, and coordination `type` values harmonizes GPT5-codex invariants with gemini’s enum strings and supports diff tooling.
- **Stable references**: inconsistent `$ref` usage made cross-file linking brittle; constraining references to relative paths with fragments supports IDE navigation and automated linting.

## Example Component Template
```yaml
format: UDML-YAML
schema_version: "0.2.0"
component:
  id: abk::agent
  name: ABK Agent Runtime
  summary: Coordinates provider calls, tool execution, checkpoints, and lifecycle bindings per session.
  kind: crate_module
  tags: [agent, orchestration, runtime]
metadata:
  owners: ["Trustee Core"]
  source_repo: "https://github.com/podtan/trustee"
  source_ref: "<commit-sha>"
  reviewed_at: "2025-11-10"
context:
  crate: abk
  features: [agent, orchestration, lifecycle, provider, checkpoint, observability]
  upstream_interfaces:
    - umf::InternalMessage
    - cats::ToolRegistry
    - abk::lifecycle::LifecycleHost
  downstream_interfaces:
    - abk::provider::LlmProvider
    - abk::checkpoint::CheckpointStore
    - cats::ToolRegistry
    - Lifecycle-WASM
domains:
  information:
    - id: session_state
      ownership: abk::agent
      summary: Aggregated runtime state for the active conversation.
      schema:
        json: |
          type: object
          required: [session_id, messages, lifecycle]
          properties:
            session_id: { type: string }
            messages:
              type: array
              items: { $ref: "../components/umf.udml.yaml#/information/internal_message" }
            lifecycle: { type: object }
            checkpoint_cursor: { type: string, nullable: true }
      lifecycle: transient
      constraints:
        - tool_results must reference an existing tool_call id
  access:
    - id: checkpoint_restore
      actors: [abk::agent, abk::checkpoint]
      mode: read_write
      scope: session_state
      summary: Restore or persist session snapshots with orchestration approval.
      authorization:
        required_roles: [orchestration-loop]
      auditing:
        log_to: abk::observability
  manipulation:
    - id: message_mutations
      summary: Append messages and tool outputs under UMF invariants.
      operations:
        - name: append_user
          inputs: { message: umf::InternalMessage }
          result: { type: void }
        - name: attach_tool_result
          inputs: { result: umf::ToolResult }
          result: { type: void }
  extract:
    - id: chatml_projection
      summary: Render ChatML transcript from UMF message history.
      inputs: [session_state.messages]
      outputs:
        - name: chatml_transcript
          schema: { type: string }
  movement:
    - id: provider_request_dispatch
      summary: Send ChatML transcript to configured LLM provider.
      source: abk::agent
      destination: abk::provider::LlmProvider
      payload:
        schema:
          type: object
          properties:
            transcript: { type: string }
            config: { $ref: "../components/abk_provider.udml.yaml#/information/generate_config" }
      protocol: rust_trait_call > wasm > https
      boundaries: [runtime, wasm, network]
  coordination:
    - id: orchestration_cycle
      summary: Core turn-based control loop across agent, provider, tools, checkpoints, and lifecycle.
      participants: [abk::agent, abk::orchestration, cats::ToolRegistry, abk::provider]
      primitives:
        - type: sequence
          description: "process inbound message -> choose action -> emit request"
        - type: retry
          description: "retry provider calls with exponential backoff"
      triggers: [message_received, checkpoint_interval]
```

The template above can be cloned across other components by adjusting metadata, context, and domain entries while preserving the standardized field set and ordering.
