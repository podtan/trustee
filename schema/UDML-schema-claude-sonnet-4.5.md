# UDML Schema Standardization: Claude Sonnet 4.5 Analysis and Proposal

## Executive Summary

After reviewing UDML YAML files across all 7 LLM implementations (claude-sonnet-4.5, GPT5, GPT5-codex, GPT5-mini, claude-sonnet-4.5-haiku, gemini-2.5-pro, grok-code-fast-1), I've identified significant variations in structure, naming conventions, and organizational patterns. This document proposes a standardized UDML schema that synthesizes the best aspects of each approach while ensuring consistency and interoperability.

## Key Findings: Variations Across Implementations

### 1. File Naming Conventions

**Variations Found:**
- **claude-sonnet-4.5**: `abk-agent.udml.yaml` (hyphen-separated, explicit `.udml.yaml`)
- **GPT5**: `abk.agent.udml.yaml` (dot-separated, explicit `.udml.yaml`)
- **GPT5-codex**: `abk_agent.udml.yaml` (underscore-separated, explicit `.udml.yaml`)
- **GPT5-mini**: `abk-agent.yaml` (hyphen-separated, no `.udml` prefix)
- **gemini-2.5-pro**: `abk_agent.yml` (underscore-separated, `.yml` extension)

**Recommendation:** Use **hyphen-separated with explicit `.udml.yaml` extension**
- Rationale: Most readable, clear intent, consistent with modern naming conventions
- Standard: `component-name.udml.yaml`

### 2. Top-Level Structure Patterns

**Four distinct patterns emerged:**

#### Pattern A: Direct Domain Keys (claude-sonnet-4.5, grok-code-fast-1)
```yaml
information:
  - name: DataShape
    description: "..."
    schema: {...}
access:
  - name: operation
    description: "..."
```

#### Pattern B: Metadata + Provides Wrapper (GPT5)
```yaml
id: component-name
layer: runtime
ownership: crate
summary: "..."
provides:
  information:
    - id: data-shape
      kind: struct
      schema: {...}
```

#### Pattern C: Format Declaration + Context (GPT5-codex)
```yaml
format: UDML-YAML-0.1
component: "component::name"
summary: "..."
context:
  crate: "..."
  feature_flags: [...]
domains:
  information: {...}
```

#### Pattern D: Metadata + Direct Domains (GPT5-mini, claude-sonnet-4.5-haiku)
```yaml
metadata:
  name: "component"
  role: "..."
  version: "1.0"
information:
  description: "..."
  canonical_schemas: {...}
```

**Recommendation:** Use **Pattern B (Metadata + Provides Wrapper)** with enhancements
- Rationale: Best balance of metadata clarity and domain organization
- GPT5's approach is most structured and machine-parseable
- Provides clear component identity before domain definitions

### 3. Domain Section Organization

**Variations in field naming:**

| Domain | claude-sonnet-4.5 | GPT5 | GPT5-codex | gemini-2.5-pro |
|--------|-------------------|------|------------|-----------------|
| Information | `name` | `id` | `name` | schema inline |
| Access | `name` | `id` | `name` | description only |
| Manipulation | `name` | `id` | `name` | description only |
| Extract | `name` | `id` | `name` | description only |
| Movement | `name` | `id` | `name` | description only |
| Coordination | `name` | `id` | `name` | description only |

**Recommendation:** Use **`id` consistently** across all domains
- Rationale: More suitable for machine processing and referencing
- Allows for clear cross-references between components

### 4. Schema Representation Approaches

**Three distinct approaches:**

#### Approach A: JSON Schema Inline (claude-sonnet-4.5)
```yaml
information:
  - name: AgentConfig
    schema:
      type: object
      properties:
        max_iterations: { type: integer }
```

#### Approach B: Structured Fields (GPT5, GPT5-codex)
```yaml
information:
  - id: session-state
    kind: struct
    schema:
      fields:
        - id: messages
          type: list<umf:internal-message>
```

#### Approach C: Rust-style Type Hints (GPT5-codex)
```yaml
information:
  - name: session_state
    shape: |
      struct SessionState {
        id: SessionId,
        messages: Vec<umf::InternalMessage>
      }
```

**Recommendation:** Support **multiple schema notations** with clear `kind` field
- Use `kind: struct` + `fields` array for structured data (GPT5 approach)
- Use `kind: schema` + JSON Schema for complex validation needs
- Use `kind: code` + inline code for Rust-native representations
- Rationale: Different use cases require different expressiveness

### 5. Reference and Dependency Syntax

**Variations Found:**
- **claude-sonnet-4.5**: `{ $ref: "#/definitions/InternalMessage" }` (JSON Schema style)
- **GPT5**: `type: list<umf:internal-message>` (Type parameter style)
- **GPT5-codex**: `Vec<umf::InternalMessage>` (Rust syntax)
- **gemini-2.5-pro**: `{ $ref: "umf.yml#InternalMessage" }` (File reference)

**Recommendation:** Use **colon-separated namespace syntax** with optional file references
- Standard: `component:entity-id` for same-repository references
- File references: `file.yaml#path/to/entity` for cross-file references
- Rationale: Clear, concise, language-agnostic

### 6. Movement/Data Flow Specifications

**Most complete implementation: GPT5**

GPT5's movement specification is the most comprehensive:
```yaml
movement:
  routes:
    - id: route-id
      direction: in|out|bi
      from: component-id
      to: component-id
      medium: memory|wasm-call|fs|network
      payload: information-id
      trigger: event|schedule|mutation
      reliability: best-effort|at-least-once|exactly-once
```

**Recommendation:** Adopt **GPT5's movement specification** as standard
- Rationale: Most complete representation of data movement semantics
- Includes reliability guarantees, triggers, and medium specifications

## Standardized UDML Schema Specification

### Core Schema Structure

```yaml
# UDML Schema v1.0 - Standardized Format
$schema: "https://udml.io/schema/v1.0"
version: "1.0"

# Component Metadata (REQUIRED)
id: component-name                    # kebab-case identifier
layer: runtime|infrastructure|boundary|support|plugin
ownership: crate|wasm|external
summary: "Single sentence component description"

# Optional Metadata
metadata:
  version: "0.1.0"                   # Semantic version
  authors: ["name"]
  created: "2025-11-10"
  updated: "2025-11-10"
  tags: ["tag1", "tag2"]

# Six UDML Domains (at least one REQUIRED)
provides:
  
  # 1. INFORMATION Domain
  information:
    - id: data-entity-id              # kebab-case, unique within component
      kind: struct|enum|alias|collection|blob|stream|schema|code
      purpose: "Why this entity exists"
      owners: [component-ids]         # Components that own this data
      
      # Schema representation (choose based on kind)
      schema:                         # For kind: struct
        fields:
          - id: field-name
            type: primitive|component:entity-id|list<type>|map<key,val>
            description: "Field purpose"
            required: boolean
            default: value
      
      # OR for kind: schema (JSON Schema)
      schema:
        type: object
        properties: {...}
        required: [...]
      
      # OR for kind: code (language-specific)
      schema:
        language: rust|python|typescript
        code: |
          struct DataEntity {
            field: Type
          }
  
  # 2. ACCESS Domain
  access:
    rules:
      - id: rule-id
        target: information-id          # Reference to information entity
        read: component-id|scope-expression
        write: component-id|scope-expression  # Optional
        constraints: [constraint-ids]
        visibility: public|private|internal
        auth: [auth-requirements]       # Optional
        description: "Access rule explanation"
  
  # 3. MANIPULATION Domain
  manipulation:
    mutations:
      - id: mutation-id
        target: information-id
        kind: create|update|delete|invalidate|correct
        operation: "operation_name(params)"  # Optional function signature
        preconditions: [rule-ids]
        postconditions: [rule-ids]
        side_effects: [movement-route-ids]
        validation: [validation-rules]
        description: "What this mutation does"
  
  # 4. EXTRACT Domain
  extract:
    transforms:
      - id: transform-id
        inputs: [information-ids]
        output: information-id
        method: algorithm|template|aggregation|projection|inference
        deterministic: boolean          # Whether output is deterministic
        cacheable: boolean              # Whether result can be cached
        description: "Transformation purpose"
        algorithm: |                    # Optional algorithm description
          detailed steps...
  
  # 5. MOVEMENT Domain
  movement:
    routes:
      - id: route-id
        direction: in|out|bi
        from: component-id|external
        to: component-id|external
        medium: memory|wasm-call|fs|network|process|stdout|stderr|ipc
        payload: information-id|stream
        protocol: http|grpc|json|binary  # Optional
        trigger: event|schedule|mutation|request
        reliability: best-effort|at-least-once|exactly-once|not-applicable
        async: boolean                  # Whether route is asynchronous
        latency: value-unit             # Optional: e.g., "10ms", "100ms"
        description: "Data movement explanation"
  
  # 6. COORDINATION Domain
  coordination:
    primitives:
      - id: coord-id
        kind: orchestration|scheduling|locking|retry|checkpoint|classification|consensus
        participants: [component-ids]
        drives: [movement-route-ids]    # Routes this primitive controls
        guarantees: [guarantee-descriptions]  # Optional
        failure_modes: [failure-descriptions]  # Optional
        description: "Coordination purpose"

# Dependencies (RECOMMENDED)
dependencies:
  runtime: [component-ids]              # Required at runtime
  build: [component-ids]                # Required at build time
  feature_flags: [flags]                # Cargo/build feature flags
  external: [crate-names]               # External crate dependencies

# Risk Assessment (OPTIONAL but RECOMMENDED)
risks:
  - id: risk-id
    category: security|performance|reliability|maintainability
    impact: low|medium|high|critical
    likelihood: low|medium|high
    description: "Risk description"
    mitigation: "Mitigation strategy"
    status: open|mitigated|accepted

# Cross-Component Integration (OPTIONAL)
integration:
  implements: [interface-ids]           # Interfaces this component implements
  extends: [component-ids]              # Components this extends
  replaces: [component-ids]             # Components this replaces/deprecates

# Examples and Usage (OPTIONAL)
examples:
  - name: "Example name"
    description: "What this example demonstrates"
    code: |
      example code or usage pattern
```

### Field Type System

#### Primitive Types
- `string`, `integer`, `float`, `boolean`, `datetime`, `duration`, `bytes`

#### Composite Types
- `list<T>` - Ordered collection
- `set<T>` - Unordered unique collection
- `map<K,V>` - Key-value mapping
- `option<T>` - Optional/nullable value
- `result<T,E>` - Success or error result

#### Reference Types
- `component:entity-id` - Reference to entity in same component
- `other-component:entity-id` - Reference to entity in another component
- `file.yaml#path/to/entity` - Reference to entity in another file

#### Advanced Types
- `stream<T>` - Streaming data
- `future<T>` - Asynchronous computation result
- `enum{variant1, variant2}` - Enumeration

### Naming Conventions

#### Component IDs
- Use kebab-case: `abk-agent`, `cats`, `umf`
- Reflect hierarchy with hyphens: `abk-checkpoint`, `provider-wasm-tanbal`

#### Entity IDs (within domains)
- Use kebab-case: `session-state`, `tool-registry`, `checkpoint-record`
- Be descriptive: prefer `session-state` over `state`

#### Field Names
- Use kebab-case in UDML: `max-iterations`, `tool-call-id`
- Use snake_case when representing Rust: `max_iterations`, `tool_call_id`

#### File Names
- Pattern: `component-name.udml.yaml`
- Examples: `abk-agent.udml.yaml`, `cats.udml.yaml`, `umf.udml.yaml`

### Required vs Optional Fields

#### REQUIRED Fields
- `id` - Component identifier
- `layer` - Architectural layer
- `ownership` - Ownership model
- `summary` - Brief description
- At least one domain section (`information`, `access`, `manipulation`, `extract`, `movement`, or `coordination`)

#### RECOMMENDED Fields
- `dependencies` - Component dependencies
- `risks` - Risk assessment
- `metadata.version` - Version tracking

#### OPTIONAL Fields
- All other metadata fields
- `integration` section
- `examples` section

## Schema Decision Rationale

### 1. Why Metadata + Provides Wrapper?

**Decision:** Use top-level metadata with `provides` wrapper for domains

**Rationale:**
- Separates component identity from domain specifications
- Makes machine parsing more straightforward
- Aligns with schema evolution (easy to add new top-level sections)
- GPT5's implementation was most structured and comprehensive

**Trade-off:** Slightly more verbose than direct domain keys, but significantly more extensible

### 2. Why `id` Instead of `name`?

**Decision:** Use `id` consistently across all entities

**Rationale:**
- Better for cross-referencing and machine processing
- Clearer semantic meaning (identifier vs human name)
- Allows future addition of `name` field for human-readable names
- Follows database and API design conventions

### 3. Why Multiple Schema Representations?

**Decision:** Support `kind` field with different schema formats

**Rationale:**
- Different use cases require different expressiveness
- Rust developers benefit from seeing Rust types directly (GPT5-codex approach)
- Complex validation requires JSON Schema (claude-sonnet-4.5 approach)
- Structured fields are best for tooling (GPT5 approach)
- Flexibility increases adoption across different team preferences

### 4. Why Adopt GPT5's Movement Specification?

**Decision:** Use GPT5's comprehensive movement route structure

**Rationale:**
- Most complete representation found across all implementations
- Includes critical fields: `reliability`, `trigger`, `medium`
- Supports distributed systems analysis
- Enables data flow diagram generation
- Critical for understanding system behavior

**Enhancement:** Added optional `protocol`, `async`, and `latency` fields for additional expressiveness

### 5. Why Add Risk Assessment Section?

**Decision:** Include optional but recommended `risks` section

**Rationale:**
- Only GPT5 included comprehensive risk tracking
- Critical for production systems
- Supports security and reliability analysis
- Encourages proactive risk thinking
- Minimal overhead when optional

### 6. Why Separate `dependencies` from Domains?

**Decision:** Create dedicated `dependencies` top-level section

**Rationale:**
- Dependencies span multiple domains
- Critical for build and deployment tooling
- Makes dependency analysis straightforward
- GPT5's approach was clearest

## Domain-Specific Guidelines

### Information Domain Guidelines

**Purpose:** Define real-world state and persistent semantic domain data

**Best Practices:**
1. Use `kind: struct` for structured data with known fields
2. Use `kind: enum` for closed sets of values
3. Use `kind: schema` for complex validation requirements
4. Always specify `owners` to clarify data ownership
5. Include `purpose` to explain why the entity exists

**Example:**
```yaml
information:
  - id: agent-session
    kind: struct
    purpose: "Tracks complete agent conversation and execution state"
    owners: [abk-agent, abk-checkpoint]
    schema:
      fields:
        - id: session-id
          type: string
          description: "Unique session identifier"
          required: true
        - id: messages
          type: list<umf:internal-message>
          description: "Conversation history"
          required: true
        - id: iteration-count
          type: integer
          description: "Number of agent turns"
          required: true
          default: 0
```

### Access Domain Guidelines

**Purpose:** Define permissible ways data may be viewed/read

**Best Practices:**
1. Specify both `read` and `write` when applicable
2. Use component IDs for `read`/`write` to show clear boundaries
3. Include `constraints` for conditional access
4. Document `visibility` for API/boundary components
5. Add `auth` requirements when security-relevant

**Example:**
```yaml
access:
  rules:
    - id: read-session-state
      target: agent-session
      read: abk-agent|abk-orchestration|abk-checkpoint
      write: abk-agent
      visibility: internal
      constraints: [valid-session-id]
      description: "Agent components can read session; only agent can write"
```

### Manipulation Domain Guidelines

**Purpose:** Define lawful mutation rules

**Best Practices:**
1. Clearly specify `kind` of mutation (create/update/delete)
2. Document `preconditions` and `postconditions`
3. List `side_effects` linking to movement routes
4. Use `validation` field for complex constraints
5. Prefer fine-grained mutations over coarse-grained ones

**Example:**
```yaml
manipulation:
  mutations:
    - id: append-message
      target: agent-session
      kind: update
      operation: "append_message(msg: InternalMessage)"
      preconditions: [session-exists, message-valid]
      postconditions: [message-in-history]
      side_effects: [mv-checkpoint-write]
      validation: [umf-schema-validation]
      description: "Append validated message to session history"
```

### Extract Domain Guidelines

**Purpose:** Define derivation of new representations

**Best Practices:**
1. Specify whether transform is `deterministic`
2. Indicate if result is `cacheable`
3. Use `method` to clarify transformation type
4. Provide algorithm description for complex transforms
5. Link input/output entities clearly

**Example:**
```yaml
extract:
  transforms:
    - id: session-to-chatml
      inputs: [agent-session]
      output: provider:chatml-request
      method: formatter
      deterministic: true
      cacheable: false
      description: "Convert UMF messages to ChatML format for provider"
      algorithm: |
        1. Iterate through session messages
        2. Convert each UMF message to ChatML format
        3. Apply role normalization
        4. Serialize to JSON
```

### Movement Domain Guidelines

**Purpose:** Define how data travels through the system

**Best Practices:**
1. Always specify `direction`, `from`, `to`, and `medium`
2. Document `reliability` guarantees explicitly
3. Use `trigger` to show what initiates data movement
4. Add `protocol` for network-crossing routes
5. Specify `async` for asynchronous operations
6. Include `latency` for performance-critical routes

**Example:**
```yaml
movement:
  routes:
    - id: mv-provider-generate
      direction: out
      from: abk-agent
      to: abk-provider
      medium: wasm-call
      payload: provider:chatml-request
      protocol: json
      trigger: event
      reliability: best-effort
      async: true
      latency: 1000ms
      description: "Send ChatML to provider for LLM generation"
```

### Coordination Domain Guidelines

**Purpose:** Define synchronization, scheduling, and orchestration

**Best Practices:**
1. Specify `kind` to clarify coordination type
2. List all `participants` explicitly
3. Link to `drives` (movement routes) that are coordinated
4. Document `guarantees` provided
5. List potential `failure_modes`

**Example:**
```yaml
coordination:
  primitives:
    - id: agent-turn-loop
      kind: orchestration
      participants: [abk-agent, abk-provider, cats, abk-checkpoint]
      drives: [mv-provider-generate, mv-tool-exec, mv-checkpoint-write]
      guarantees: [ordered-message-sequence, checkpoint-consistency]
      failure_modes: [provider-timeout, tool-failure, checkpoint-write-failure]
      description: "Orchestrate one complete agent turn: classify, generate, execute tools, checkpoint"
```

## Example: Complete UDML File

Here's a complete example following the standardized schema for a simplified `cats` component:

```yaml
# UDML Schema v1.0 - Standardized Format
$schema: "https://udml.io/schema/v1.0"
version: "1.0"

# Component Metadata
id: cats
layer: runtime
ownership: crate
summary: "Code Agent Tool System providing LLM-facing tools and execution registry"

metadata:
  version: "0.1.2"
  authors: ["Trustee Team"]
  created: "2025-11-10"
  tags: ["tools", "execution", "agent-interface"]

# Six UDML Domains
provides:
  
  information:
    - id: tool-registry
      kind: struct
      purpose: "Central registry of all available tools with their contracts"
      owners: [cats]
      schema:
        fields:
          - id: tools
            type: map<string, tool-spec>
            description: "Tool name to specification mapping"
            required: true
          - id: categories
            type: map<string, list<string>>
            description: "Category to tool names mapping"
            required: false
    
    - id: tool-spec
      kind: struct
      purpose: "Contract and metadata for a single tool"
      owners: [cats]
      schema:
        fields:
          - id: name
            type: string
            required: true
          - id: description
            type: string
            required: true
          - id: input-schema
            type: schema
            description: "JSON Schema for tool input"
            required: true
          - id: output-schema
            type: schema
            description: "JSON Schema for tool output"
            required: true
          - id: safety-level
            type: enum{safe, moderate, dangerous}
            required: true
            default: moderate
    
    - id: tool-result
      kind: struct
      purpose: "Normalized result from tool execution consumable by agent"
      owners: [cats, abk-agent]
      schema:
        fields:
          - id: tool-name
            type: string
            required: true
          - id: success
            type: boolean
            required: true
          - id: stdout
            type: string
            required: false
          - id: stderr
            type: string
            required: false
          - id: exit-code
            type: integer
            required: false
          - id: execution-time-ms
            type: integer
            required: true
  
  access:
    rules:
      - id: registry-read
        target: tool-registry
        read: abk-agent|abk-orchestration
        write: cats
        visibility: public
        description: "Agent and orchestration can read registry to resolve tool calls"
      
      - id: tool-result-read
        target: tool-result
        read: abk-agent|abk-orchestration|umf
        write: cats
        visibility: public
        description: "Execution results available to agent workflow"
  
  manipulation:
    mutations:
      - id: register-tool
        target: tool-registry
        kind: create
        operation: "register_tool(spec: ToolSpec)"
        preconditions: [tool-name-unique]
        postconditions: [tool-in-registry]
        description: "Add new tool to registry"
      
      - id: execute-tool
        target: tool-result
        kind: create
        operation: "execute_tool(call: ToolCall) -> ToolResult"
        preconditions: [tool-exists, params-valid]
        postconditions: [result-created]
        side_effects: [mv-exec]
        validation: [input-schema-validation]
        description: "Execute tool and return normalized result"
  
  extract:
    transforms:
      - id: tool-call-adapter
        inputs: [umf:tool-call]
        output: abk-executor:exec-request
        method: adapter
        deterministic: true
        cacheable: false
        description: "Convert UMF tool call to executor request for command tools"
      
      - id: analyze-tool-usage
        inputs: [list<tool-result>]
        output: tool-usage-stats
        method: aggregation
        deterministic: true
        cacheable: true
        description: "Aggregate tool execution statistics for observability"
  
  movement:
    routes:
      - id: mv-exec
        direction: out
        from: cats
        to: abk-executor
        medium: process
        payload: abk-executor:exec-request
        trigger: event
        reliability: best-effort
        async: true
        description: "Execute command-based tools via executor"
      
      - id: mv-tool-call-in
        direction: in
        from: abk-agent
        to: cats
        medium: memory
        payload: umf:tool-call
        trigger: event
        reliability: best-effort
        async: false
        description: "Receive tool invocation from agent"
  
  coordination:
    primitives:
      - id: tool-execution-policy
        kind: orchestration
        participants: [cats, abk-config, abk-executor]
        drives: [mv-exec]
        guarantees: [tool-timeout-enforcement, safety-level-respect]
        failure_modes: [tool-timeout, tool-crash, invalid-parameters]
        description: "Enforce configuration-based tool policies and timeouts"

dependencies:
  runtime: [abk-executor, abk-config, umf]
  build: [cats]
  feature_flags: []
  external: [serde, serde_json]

risks:
  - id: dangerous-tool-execution
    category: security
    impact: high
    likelihood: medium
    description: "Tools may execute arbitrary commands or modify filesystem"
    mitigation: "Safety level classification, sandboxing, validation, configurable tool policies"
    status: mitigated
  
  - id: tool-timeout-deadlock
    category: reliability
    impact: medium
    likelihood: low
    description: "Long-running tools may block agent progress"
    mitigation: "Timeout enforcement in executor, async execution pattern"
    status: mitigated

integration:
  implements: [tool-provider-interface]
  extends: []

examples:
  - name: "Basic tool execution"
    description: "Register and execute a simple file operation tool"
    code: |
      # Register tool
      let spec = ToolSpec {
        name: "read_file",
        description: "Read contents of a file",
        input_schema: {...},
        safety_level: Safe
      };
      registry.register_tool(spec);
      
      # Execute tool
      let call = ToolCall {
        name: "read_file",
        parameters: { "path": "config.toml" }
      };
      let result = registry.execute_tool(call);
```

## Implementation Recommendations

### For Tool Developers

1. **Start with metadata**: Define `id`, `layer`, `ownership`, and `summary` first
2. **Model information entities**: Begin with the `information` domain
3. **Define access patterns**: Document how entities are read/written in `access`
4. **Specify mutations**: Use `manipulation` to define state changes
5. **Map data flow**: Use `movement` to show how data travels
6. **Add coordination last**: Define `coordination` for complex orchestration

### For Schema Validators

1. Validate required fields: `id`, `layer`, `ownership`, `summary`
2. Check at least one domain section exists
3. Validate cross-references (all referenced IDs exist)
4. Verify `kind` matches schema structure
5. Check `movement` routes reference valid components
6. Validate `coordination` participants exist

### For Code Generators

1. Parse `information` domain for type definitions
2. Use `kind` field to determine code generation strategy
3. Generate interfaces from `access` rules
4. Create mutation functions from `manipulation` operations
5. Generate data flow diagrams from `movement` routes
6. Create orchestration code from `coordination` primitives

### For Documentation Generators

1. Use `summary` for brief component descriptions
2. Extract domain sections for detailed documentation
3. Generate API docs from `access` and `manipulation`
4. Create architecture diagrams from `movement` and `coordination`
5. Include `examples` in generated documentation
6. Document `risks` in security/reliability sections

## Migration Path from Existing Implementations

### For claude-sonnet-4.5 files:
1. Add top-level `id`, `layer`, `ownership` metadata
2. Wrap domains in `provides` section
3. Change `name` to `id` for all entities
4. No schema changes needed (already uses good structure)

### For GPT5 files:
1. Minimal changes needed (closest to standard)
2. Consider adding `risks` section
3. Verify `movement` routes include all recommended fields

### For GPT5-codex files:
1. Rename `domains` to `provides`
2. Change `name` to `id` for entities
3. Standardize top-level metadata structure
4. Optionally keep Rust code in `kind: code` schemas

### For GPT5-mini files:
1. Add `.udml` to file extension
2. Add `layer`, `ownership` to metadata
3. Expand abbreviated domain sections
4. Add `id` fields to entities

### For gemini-2.5-pro files:
1. Change `.yml` to `.udml.yaml`
2. Expand abbreviated structure to full schema
3. Add detailed entity definitions
4. Standardize cross-references

## Conclusion

The standardized UDML schema proposed here synthesizes the best aspects of all seven LLM implementations:

- **Structure**: GPT5's metadata + provides wrapper
- **Movement**: GPT5's comprehensive route specification  
- **Risk Assessment**: GPT5's risk tracking
- **Schema Flexibility**: Support for multiple representations (GPT5, GPT5-codex, claude-sonnet-4.5)
- **Naming**: Consistent kebab-case with hyphen-separated filenames
- **Completeness**: claude-sonnet-4.5's detailed information models

This schema balances:
- **Human readability** (YAML with comments)
- **Machine parseability** (structured, consistent)
- **Expressiveness** (multiple schema kinds)
- **Extensibility** (metadata separation)
- **Interoperability** (standard reference format)

By adopting this standardized schema, UDML implementations across different tools and LLMs will be consistent, enabling better tooling, validation, code generation, and cross-component analysis.
