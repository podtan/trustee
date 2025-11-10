```markdown
# Standardized UDML Schema Proposal (Gemini 2.5 Pro)

This document presents a standardized UDML schema based on a comprehensive review of implementations from seven different LLMs. Our goal is to establish a consistent, interoperable, and clear standard for describing software components using the Universal Domain Modeling Language (UDML).

## 1. Summary of Variations Found

Our analysis of the UDML YAML files revealed several key areas of inconsistency across the different LLM implementations:

- **Naming Conventions**:
  - **Component Identification**: Field names for the primary identifier varied, including `id`, `name`, `component_name`, and `componentId`.
  - **Descriptive Fields**: Descriptions were labeled as `description`, `summary`, `overview`, or `purpose`.
  - **Case Styles**: We observed a mix of `camelCase`, `snake_case`, and `kebab-case` for field names.

- **Structural Inconsistencies**:
  - **Nesting Depth**: The level of nesting for attributes like `properties` or `rules` was inconsistent. Some implementations used a flat structure, while others preferred deep nesting.
  - **Data Grouping**: The use of lists versus maps for collections of related items (e.g., access rules, manipulation operations) varied significantly.
  - **Domain Specification**: The way a component was assigned to a UDML domain was not uniform. Some used a top-level `domain` key, while others nested it within a metadata object.

- **Data Type and Format Variations**:
  - **Booleans**: Boolean values were represented as native `true`/`false`, strings `"true"`/`"false"`, or even integers `1`/`0`.
  - **Enums**: Fields with a limited set of possible values (like access levels) often lacked a strict, predefined list, leading to variations like `"read"`, `"read-only"`, and `"viewer"`.
  - **Metadata**: The inclusion and structure of metadata fields were ad-hoc, with no consistent schema for capturing version, author, or timestamp.

- **Granularity and Detail**:
  - The level of detail in descriptions ranged from a single sentence to multiple paragraphs.
  - The breakdown of complex operations (e.g., in the **Manipulation** domain) was inconsistent, with some LLMs providing a high-level overview and others detailing every sub-step.

## 2. Standardized UDML Schema Specification

To address these variations, we propose the following standardized schema. This schema is designed to be clear, consistent, and expressive.

### Root Schema

All UDML files MUST begin with this root structure.

- `udmlVersion` (string, required): The version of the UDML specification used. (e.g., "1.0")
- `componentId` (string, required): A unique, machine-readable identifier for the component (e.g., "abk-agent").
- `displayName` (string, required): A human-readable name for the component (e.g., "Agent Builder Kit - Agent").
- `description` (string, required): A concise, one-paragraph summary of the component's purpose.
- `domain` (enum, required): The primary UDML domain the component belongs to. Must be one of: `Information`, `Access`, `Manipulation`, `Extract`, `Movement`, `Coordination`.
- `spec` (object, required): A domain-specific object containing the detailed specification.

---

### Domain-Specific Schemas (`spec` object)

#### A. Information Domain

- `entities` (array[object], required): A list of primary data entities managed by the component.
  - `entityId` (string, required): Unique ID for the entity.
  - `description` (string, required): Description of the entity.
  - `schema` (object, required): A map of attributes, where each key is the attribute name.
    - `type` (string, required): Data type (e.g., `string`, `integer`, `boolean`, `timestamp`, `object`, `array`).
    - `description` (string, required): Description of the attribute.
    - `required` (boolean, optional, default: `false`).

#### B. Access Domain

- `boundaries` (array[object], required): Defines the access boundaries.
  - `boundaryId` (string, required): Unique ID for the boundary.
  - `description` (string, required): What this boundary protects.
  - `rules` (array[object], required): Access control rules.
    - `principal` (string, required): The actor the rule applies to (e.g., "user", "admin", "service:auth").
    - `permissions` (array[string], required): List of allowed actions (e.g., `read`, `list`, `query`).
    - `condition` (string, optional): A conditional expression for the rule.

#### C. Manipulation Domain

- `operations` (array[object], required): A list of state-changing operations.
  - `operationId` (string, required): Unique ID for the operation (e.g., `create-session`).
  - `description` (string, required): What the operation does.
  - `verb` (enum, required): The type of manipulation (`create`, `update`, `delete`, `invalidate`, `correct`).
  - `preconditions` (array[string], optional): Conditions that must be met before execution.
  - `postconditions` (array[string], optional): State guarantees after successful execution.

#### D. Extract Domain

- `projections` (array[object], required): Describes derived data representations.
  - `projectionId` (string, required): Unique ID for the projection.
  - `description` (string, required): The purpose of the derived data.
  - `sourceEntities` (array[string], required): The source `entityId`s used.
  - `logic` (string, required): A high-level description of the extraction logic (e.g., "Aggregates user events into a daily summary").
  - `outputSchema` (object, required): The schema of the derived data, following the `Information.entities.schema` format.

#### E. Movement Domain

- `flows` (array[object], required): Defines how data moves.
  - `flowId` (string, required): Unique ID for the data flow.
  - `description` (string, required): The purpose and path of the flow.
  - `source` (string, required): The origin of the data (e.g., `component:abk-provider`).
  - `destination` (string, required): The target of the data (e.g., `external:openai-api`).
  - `protocol` (string, required): The communication protocol (e.g., `https`, `grpc`, `wasm-abi`).
  - `payloadSchema` (object, optional): The schema of the data being moved.

#### F. Coordination Domain

- `primitives` (array[object], required): Defines synchronization and orchestration mechanisms.
  - `primitiveId` (string, required): Unique ID for the primitive (e.g., `session-locking`).
  - `type` (enum, required): The type of primitive (`lock`, `semaphore`, `scheduler`, `dependency-graph`, `consensus`).
  - `description` (string, required): The role of the primitive.
  - `strategy` (string, optional): The specific algorithm or strategy used (e.g., "optimistic-locking", "round-robin-scheduling").

## 3. Rationale for Schema Decisions

- **Standardized Root**: A consistent root structure (`udmlVersion`, `componentId`, etc.) ensures every UDML file is immediately identifiable and parsable. `componentId` was chosen for machine-readability, while `displayName` provides a human-friendly label.
- **`spec` Object**: Placing all domain-specific attributes under a `spec` object creates a clear separation between universal metadata and the detailed, domain-specific implementation. This improves modularity and readability.
- **Consistent IDs**: Using `[domainEntity]Id` (e.g., `entityId`, `operationId`) as a convention for all primary keys within arrays provides predictable and unique identification for referencing.
- **Strict Enums**: Defining strict `enum` types for fields like `domain` and `verb` eliminates ambiguity and ensures consistency across all models.
- **Descriptive Clarity**: Requiring a `description` field for all major objects enforces a minimum level of documentation and improves human understanding.
- **Array of Objects**: Using an array of objects (e.g., `entities`, `operations`) is more extensible and queryable than a map, and it allows for a richer set of attributes for each item.

## 4. Example Implementations

### Information (abk-checkpoint)
```yaml
udmlVersion: "1.0"
componentId: "abk-checkpoint"
displayName: "ABK - Checkpoint"
description: "Manages session state persistence and recovery."
domain: Information
spec:
  entities:
    - entityId: "session-state"
      description: "A snapshot of the agent's state at a point in time."
      schema:
        - attributeId: "sessionId"
          type: "string"
          description: "Unique identifier for the session."
          required: true
        - attributeId: "history"
          type: "array"
          description: "Conversation history."
        - attributeId: "createdAt"
          type: "timestamp"
          description: "When the checkpoint was created."
```

### Access (abk-provider)
```yaml
udmlVersion: "1.0"
componentId: "abk-provider"
displayName: "ABK - Provider"
description: "Abstracts interactions with LLM providers."
domain: Access
spec:
  boundaries:
    - boundaryId: "api-key-access"
      description: "Protects access to provider API keys."
      rules:
        - principal: "service:abk-agent"
          permissions: ["read"]
          condition: "is_internal_request()"
        - principal: "user"
          permissions: []
```

### Manipulation (cats-file-editing)
```yaml
udmlVersion: "1.0"
componentId: "cats-file-editing"
displayName: "CATS - File Editing Tools"
description: "Provides tools for manipulating file content."
domain: Manipulation
spec:
  operations:
    - operationId: "replace-text"
      description: "Replaces a specific string in a file."
      verb: "update"
      preconditions: ["file_exists", "file_is_writable"]
      postconditions: ["file_content_updated", "timestamp_modified"]
```

### Extract (umf-streaming)
```yaml
udmlVersion: "1.0"
componentId: "umf-streaming"
displayName: "UMF - Streaming"
description: "Parses and extracts structured data from streaming responses."
domain: Extract
spec:
  projections:
    - projectionId: "sse-to-chatml"
      description: "Extracts ChatML messages from a Server-Sent Events (SSE) stream."
      sourceEntities: ["raw-sse-event"]
      logic: "Parses 'data' field of SSE events, deserializes JSON, and constructs a ChatML message."
      outputSchema:
        - attributeId: "role"
          type: "string"
        - attributeId: "content"
          type: "string"
```

### Movement (provider-wasm-tanbal)
```yaml
udmlVersion: "1.0"
componentId: "provider-wasm-tanbal"
displayName: "Tanbal WASM Provider"
description: "Routes LLM requests from the agent to the correct backend via WASM."
domain: Movement
spec:
  flows:
    - flowId: "request-to-openai"
      description: "Forwards a generation request from the agent to the OpenAI API."
      source: "component:abk-agent"
      destination: "external:api.openai.com"
      protocol: "https"
      payloadSchema:
        - attributeId: "model"
          type: "string"
        - attributeId: "messages"
          type: "array"
```

### Coordination (abk-orchestration)
```yaml
udmlVersion: "1.0"
componentId: "abk-orchestration"
displayName: "ABK - Orchestration"
description: "Coordinates the overall workflow and session management."
domain: Coordination
spec:
  primitives:
    - primitiveId: "task-execution-lock"
      type: "lock"
      description: "Ensures that only one tool execution or LLM call is active at a time within a session."
      strategy: "mutex"
```
```