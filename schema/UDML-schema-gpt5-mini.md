## UDML schema proposal — compact for GPT5-mini

This document summarizes variations found across the UDML component YAMLs and proposes a compact, standardized UDML schema tailored for the GPT5-mini component generation pipeline. The focus is clarity, interoperability, and small-model-friendliness (concise field names, explicit required/optional classification, and small examples). Use this as a canonical target for automated generation and validation.

---

### 1) Summary of variations observed (high level)
- Field-name inconsistencies: `description` vs `desc`, `id` vs `name`, `inputs` vs `parameters`.
- Structural differences: some components nest capabilities under `capabilities` while others inline them as `features`.
- Typing omissions: fields often lacked explicit types (string/enum/array/object) or optionality metadata.
- Domain mixing: technical runtime metadata (e.g., `executable`, `path`) mixed with higher-level `Information` meta fields.
- Naming conventions varied between snake_case, camelCase, and kebab-case.

These variations reduce predictable parsing and make small models guess field semantics. The standardized schema below picks explicit names, types, and minimal nesting to reduce cognitive load for GPT5-mini.

---

### 2) Small contract (inputs / outputs / success)
- Input: component metadata and domain classification (one of: Information, Access, Manipulation, Extract, Movement, Coordination).
- Output: a UDML YAML document that conforms to the schema below.
- Success: YAML validates against the rules and sample templates and yields consistent field names across components.

Edge cases: missing optional fields, empty arrays, mixed-case names — validator must normalize names to the chosen convention (snake_case) and reject unknown required fields.

---

## 3) Standardized compact UDML schema (GPT5-mini friendly)
Use snake_case naming. Keep top-level fields predictable and shallow (<= 2 levels where possible).

Top-level manifest (required):
- `udml_version` (string) — e.g. "1.0"
- `component_id` (string) — stable identifier, ASCII, kebab or snake allowed but stored as snake_case
- `component_type` (enum) — one of: `agent`, `library`, `service`, `template`, `connector`
- `domain` (enum) — one of: `information`, `access`, `manipulation`, `extract`, `movement`, `coordination`
- `title` (string)
- `description` (string)

Top-level manifest (optional):
- `tags` (array[string])
- `version` (string)
- `authors` (array[string])
- `license` (string)

Behavior / capabilities (recommended):
- `capabilities` (array[capability]) — capability is an object with:
  - `id` (string) — short id
  - `title` (string)
  - `description` (string)
  - `inputs` (array[input_spec]) optional
  - `outputs` (array[output_spec]) optional
  - `constraints` (object) optional

Input / output spec (common shape):
- `name` (string)
- `type` (enum|string) — `string`, `integer`, `float`, `boolean`, `object`, `array`, or custom type name
- `required` (boolean)
- `description` (string) optional
- `example` (any) optional

Runtime metadata (optional, keep minimal):
- `runtime` (object):
  - `exec` (string) — short token like `wasm`, `cli`, `http`, `none`
  - `endpoint` (string) optional — URL or path
  - `protocol` (string) optional — `http`, `grpc`, `sse`, etc.

Interoperability hints (optional):
- `mapping` (object) — maps this component fields to other schemas (keys are external schema names, values are local fields)

Formalized required vs optional summary:
- Required: `udml_version`, `component_id`, `component_type`, `domain`, `title`, `description`
- Optional: `tags`, `version`, `authors`, `license`, `capabilities`, `runtime`, `mapping`

Naming conventions & formats:
- Use snake_case for keys everywhere.
- Strings: UTF-8, prefer short, human-friendly text (<= 240 chars for `title`, <= 2000 chars for `description`).
- IDs: ASCII letters, numbers, underscores. Prefer `component_id` like `abk_agent` or `cats_tool_cli`.

---

## 4) Rationale for major decisions
- Snake_case: small models handle consistent tokenization better; it's unambiguous.
- Explicit types for inputs/outputs: reduces hallucination about field meaning and supports simple validation.
- Shallow nesting: GPT5-mini prefers flatter structures to avoid long-range dependency errors.
- Fixed domain enum: gives deterministic routing of downstream processing pipelines.
- Separate `runtime` vs `capabilities`: separates declarative metadata from execution concerns.

---

## 5) Domain-specific guidance and examples
Below are compact UDML examples for each UDML domain using the standardized schema. Keep examples intentionally minimal so GPT5-mini can both generate and validate them easily.

### Information (real-world state / persistent semantic data)
Example:

```yaml
udml_version: "1.0"
component_id: abk_information_store
component_type: service
domain: information
title: ABK Information Store
description: Stores agent persistent metadata and semantic indexes.
version: "0.1"
capabilities:
  - id: read_entity
    title: Read Entity
    description: Retrieve an entity by id
    inputs:
      - name: entity_id
        type: string
        required: true
        description: Unique entity id
    outputs:
      - name: entity
        type: object
        required: true
        description: Serialized entity object
```

### Access (viewing / queries / boundaries)
```yaml
udml_version: "1.0"
component_id: abk_access_layer
component_type: service
domain: access
title: ABK Access Layer
description: Read-only access endpoints and index definitions.
capabilities:
  - id: query_index
    title: Query Index
    inputs:
      - name: q
        type: string
        required: true
    outputs:
      - name: results
        type: array
        required: true
```

### Manipulation (create/update/delete rules)
```yaml
udml_version: "1.0"
component_id: abk_mutation_api
component_type: service
domain: manipulation
title: ABK Mutation API
description: Rules and endpoints for lawful mutations.
capabilities:
  - id: create_checkpoint
    title: Create Checkpoint
    inputs:
      - name: payload
        type: object
        required: true
    outputs:
      - name: checkpoint_id
        type: string
        required: true
```

### Extract (inference, aggregation)
```yaml
udml_version: "1.0"
component_id: abk_extractor
component_type: service
domain: extract
title: ABK Extractor
description: Derive aggregated summaries and inferences.
capabilities:
  - id: summarize
    title: Summarize Data
    inputs:
      - name: documents
        type: array
        required: true
    outputs:
      - name: summary
        type: string
        required: true
```

### Movement (how data moves across boundaries)
```yaml
udml_version: "1.0"
component_id: abk_transfer
component_type: connector
domain: movement
title: ABK Transfer Connector
description: Defines pipelines for data movement.
runtime:
  exec: http
  endpoint: https://transfer.example/api
capabilities:
  - id: push_data
    title: Push Data
    inputs:
      - name: payload
        type: object
        required: true
```

### Coordination (scheduling, orchestration)
```yaml
udml_version: "1.0"
component_id: abk_orchestration
component_type: service
domain: coordination
title: ABK Orchestration
description: Scheduling and dependency resolution primitives.
capabilities:
  - id: schedule_task
    title: Schedule Task
    inputs:
      - name: task_spec
        type: object
        required: true
    outputs:
      - name: schedule_id
        type: string
        required: true
```

---

## 6) Quick validator guidance (for small models)
- Validate required top-level fields exist.
- Normalize keys to snake_case before schema validation.
- Validate `domain` and `component_type` against enums.
- Validate every `capability` input/output has `name` + `type` and `required` field.
- Minimal error messages — return a short list of missing/invalid fields.

---

## 7) Next steps & adoption notes
- Adopt this compact schema as the canonical target for generator prompts for `GPT5-mini` and other small models.
- Add a CI check that validates UDML YAMLs against this schema; keep validator lightweight (a small Python/Rust script that checks keys and simple types).
- For existing YAMLs, run a migration script that maps common synonyms (`desc`→`description`, `parameters`→`inputs`, `features`→`capabilities`).

---

## Appendix: short mapping table (common synonyms)
- `desc` -> `description`
- `name` (when ambiguous) -> if top-level use `component_id`, else `title`
- `parameters` -> `inputs`
- `results` -> `outputs`

---

This file is intentionally compact and pragmatic for small-model generation. If you want, I can also generate a JSON Schema / OpenAPI fragment (small) for the validator and a one-line CLI validation script example.
