# UDML Guide: Universal Data Morphism Language

## Introduction

The Universal Data Morphism Language (UDML) is a standardized format for describing software components through the lens of data and its transformations. UDML posits that all software can be fully described by defining data and the rules governing how data is accessed, manipulated, extracted, moved, and coordinated.

## Core Philosophy

> **The only truly fundamental primitive in software is data, and all software is the controlled morphing of data.**

UDML eliminates the need for behavior-first or architecture-first descriptions. Instead, it models systems as compositions of:
- **Information** (what data exists)
- **Access** (who can view it)
- **Manipulation** (how it changes)
- **Extract** (how new data derives from existing data)
- **Movement** (how data flows)
- **Coordination** (how operations synchronize)

## Document Structure

Every UDML file follows this top-level structure:

```yaml
$schema: "https://udml.podtan.com/schema/v0.1"
version: "0.1"

# Component identification
id: component-name
layer: runtime
ownership: crate
summary: "Brief component description"

# Optional metadata
metadata:
  version: "0.1.0"
  authors: ["Team Name"]
  created: "2025-11-10"
  updated: "2025-11-10"
  tags: ["tag1", "tag2"]

# The six UDML domains
provides:
  information: []
  access: []
  manipulation: []
  extract: []
  movement: []
  coordination: []

# Supporting sections
dependencies: {}
risks: []
integration: {}
examples: []
```

## The Six UDML Domains

### 1. Information Domain

**Purpose:** Define real-world state and persistent semantic domain data.

**When to use:** When you need to describe data structures, entities, schemas, or persistent state.

**Structure:**

```yaml
information:
  - id: user-session
    kind: struct
    purpose: "Tracks user conversation and execution state"
    owners: [agent-service, checkpoint-service]
    schema:
      fields:
        - id: session-id
          type: string
          description: "Unique session identifier"
          required: true
        - id: user-id
          type: string
          description: "User who owns this session"
          required: true
        - id: messages
          type: list<message>
          description: "Conversation history"
          required: true
        - id: created-at
          type: datetime
          description: "Session creation timestamp"
          required: true
        - id: status
          type: enum{active, paused, completed, expired}
          description: "Current session state"
          required: true
          default: active
```

**Field Types:**

- **Primitives:** `string`, `integer`, `float`, `boolean`, `datetime`, `duration`, `bytes`
- **Collections:** `list<T>`, `set<T>`, `map<K,V>`
- **References:** `component:entity-id` (e.g., `auth:user-profile`)
- **Advanced:** `stream<T>`, `option<T>`, `enum{val1, val2}`

**Kind Options:**

- `struct` - Structured data with defined fields
- `enum` - Enumeration of possible values
- `schema` - JSON Schema for complex validation
- `code` - Language-specific type definition
- `alias` - Type alias/wrapper
- `collection` - Collection without internal structure
- `blob` - Opaque binary data
- `stream` - Streaming data

### 2. Access Domain

**Purpose:** Define permissible ways data may be viewed or read.

**When to use:** When specifying who can read/write data, authentication requirements, or visibility boundaries.

**Structure:**

```yaml
access:
  rules:
    - id: read-user-session
      target: user-session
      read: agent-service|checkpoint-service|admin-dashboard
      write: agent-service
      visibility: internal
      constraints: [valid-session-token]
      auth: [bearer-token]
      description: "Agent and checkpoint can read; only agent can write"
    
    - id: read-message-history
      target: user-session
      read: user-id-match
      visibility: private
      constraints: [user-owns-session]
      description: "Users can only read their own message history"
```

**Visibility Levels:**

- `public` - Accessible to anyone
- `internal` - Accessible within system boundaries
- `private` - Restricted to specific components/users
- `restricted` - Highly restricted access

**Auth Methods:** `bearer-token`, `api-key`, `oauth2`, `mutual-tls`, `basic-auth`

### 3. Manipulation Domain

**Purpose:** Define lawful mutation rules for data creation, modification, and deletion.

**When to use:** When describing state-changing operations with their preconditions and effects.

**Structure:**

```yaml
manipulation:
  mutations:
    - id: create-session
      target: user-session
      kind: create
      operation: "create_session(user_id: string) -> session-id"
      preconditions:
        - user-authenticated
        - no-active-session-for-user
      postconditions:
        - session-exists
        - session-status-active
      side_effects:
        - mv-notify-session-created
      validation:
        - user-id-format-valid
      description: "Create new user session with initial state"
    
    - id: append-message
      target: user-session
      kind: update
      operation: "append_message(session_id: string, message: message)"
      preconditions:
        - session-exists
        - session-active
        - message-valid
      postconditions:
        - message-in-history
        - session-updated-timestamp
      side_effects:
        - mv-checkpoint-trigger
      description: "Append message to session history"
    
    - id: expire-session
      target: user-session
      kind: update
      operation: "expire_session(session_id: string)"
      preconditions:
        - session-exists
        - session-inactive-timeout-exceeded
      postconditions:
        - session-status-expired
      side_effects:
        - mv-cleanup-session-data
      description: "Mark inactive session as expired"
```

**Mutation Kinds:**

- `create` - Create new data
- `update` - Modify existing data
- `delete` - Remove data
- `invalidate` - Mark data as invalid without deletion
- `correct` - Fix incorrect data

### 4. Extract Domain

**Purpose:** Define derivation of new representations from existing data.

**When to use:** When describing data transformations, aggregations, computations, or inferences.

**Structure:**

```yaml
extract:
  transforms:
    - id: session-summary
      inputs: [user-session]
      output: session-summary-report
      method: aggregation
      deterministic: true
      cacheable: true
      description: "Generate summary statistics from session"
      algorithm: |
        1. Count total messages
        2. Calculate session duration
        3. Identify message types distribution
        4. Extract key topics
        5. Generate summary report
    
    - id: user-intent-classification
      inputs: [user-session]
      output: intent-classification
      method: inference
      deterministic: false
      cacheable: false
      description: "Classify user intent from conversation history"
      algorithm: |
        1. Extract last N messages
        2. Apply NLP classification model
        3. Return intent with confidence score
    
    - id: format-for-display
      inputs: [user-session]
      output: display-session
      method: projection
      deterministic: true
      cacheable: true
      description: "Project session data to user-facing format"
      algorithm: |
        1. Select user-visible fields
        2. Format timestamps for timezone
        3. Sanitize sensitive data
        4. Apply UI formatting rules
```

**Transform Methods:**

- `algorithm` - Computational transformation
- `template` - Template-based formatting
- `aggregation` - Statistical aggregation
- `projection` - Field selection/filtering
- `inference` - ML/AI-based derivation

### 5. Movement Domain

**Purpose:** Define how data travels through processes, boundaries, and protocols.

**When to use:** When describing data flow between components, external systems, or across boundaries.

**Structure:**

```yaml
movement:
  routes:
    - id: mv-session-to-checkpoint
      direction: out
      from: agent-service
      to: checkpoint-service
      medium: network
      payload: user-session
      protocol: grpc
      trigger: event
      reliability: at-least-once
      async: true
      latency: 50ms
      description: "Send session state to checkpoint service for persistence"
    
    - id: mv-message-from-user
      direction: in
      from: external
      to: agent-service
      medium: network
      payload: user-message
      protocol: https
      trigger: request
      reliability: exactly-once
      async: false
      latency: 10ms
      description: "Receive user message via HTTPS API"
    
    - id: mv-notify-session-created
      direction: out
      from: agent-service
      to: notification-service
      medium: process
      payload: session-created-event
      protocol: json
      trigger: mutation
      reliability: best-effort
      async: true
      description: "Notify other services of new session creation"
    
    - id: mv-log-to-file
      direction: out
      from: agent-service
      to: external
      medium: fs
      payload: log-entry
      protocol: binary
      trigger: event
      reliability: best-effort
      async: true
      description: "Write log entries to filesystem"
```

**Direction:** `in` (incoming), `out` (outgoing), `bi` (bidirectional)

**Medium:**
- `memory` - In-memory function call
- `wasm-call` - WebAssembly boundary
- `fs` - Filesystem
- `network` - Network communication
- `process` - Inter-process communication
- `stdout`/`stderr` - Standard output/error
- `ipc` - Inter-process communication

**Reliability Guarantees:**
- `best-effort` - No delivery guarantee
- `at-least-once` - May deliver multiple times
- `exactly-once` - Delivers exactly once
- `not-applicable` - Reliability not relevant

**Triggers:**
- `event` - Triggered by events
- `schedule` - Triggered by schedule
- `mutation` - Triggered by data mutation
- `request` - Triggered by request

### 6. Coordination Domain

**Purpose:** Define synchronization, scheduling, and orchestration primitives.

**When to use:** When describing how operations coordinate, synchronize, or orchestrate.

**Structure:**

```yaml
coordination:
  primitives:
    - id: session-lifecycle-orchestration
      kind: orchestration
      participants:
        - agent-service
        - checkpoint-service
        - notification-service
      drives:
        - mv-session-to-checkpoint
        - mv-notify-session-created
      guarantees:
        - session-consistency
        - ordered-message-processing
      failure_modes:
        - checkpoint-service-unavailable
        - notification-delivery-failed
      description: "Orchestrate complete session lifecycle from creation to expiration"
    
    - id: checkpoint-scheduling
      kind: scheduling
      participants:
        - checkpoint-service
      drives:
        - mv-session-to-checkpoint
      guarantees:
        - periodic-checkpoint-execution
      description: "Schedule periodic session checkpointing every 5 minutes"
    
    - id: session-lock
      kind: locking
      participants:
        - agent-service
      drives: []
      guarantees:
        - no-concurrent-session-modification
      failure_modes:
        - deadlock-detection
        - lock-timeout
      description: "Prevent concurrent modifications to same session"
    
    - id: retry-checkpoint-write
      kind: retry
      participants:
        - checkpoint-service
      drives:
        - mv-session-to-checkpoint
      guarantees:
        - eventual-persistence
      description: "Retry checkpoint writes with exponential backoff"
```

**Coordination Kinds:**

- `orchestration` - Multi-step workflow coordination
- `scheduling` - Time-based execution
- `locking` - Mutual exclusion
- `retry` - Retry logic
- `checkpoint` - State checkpointing
- `classification` - Request routing/classification
- `consensus` - Distributed agreement

## Complete Example: User Session Service

Here's a complete UDML specification for a user session management service:

```yaml
$schema: "https://udml.podtan.com/schema/v0.1"
version: "0.1"

id: user-session-service
layer: runtime
ownership: crate
summary: "Manages user conversation sessions with persistence and state management"

metadata:
  version: "1.0.0"
  authors: ["Platform Team"]
  created: "2025-11-10"
  updated: "2025-11-10"
  tags: ["sessions", "state-management", "persistence"]

provides:
  
  information:
    - id: user-session
      kind: struct
      purpose: "Complete user conversation state including messages and metadata"
      owners: [user-session-service, checkpoint-service]
      schema:
        fields:
          - id: session-id
            type: string
            description: "Unique session identifier (ULID)"
            required: true
          - id: user-id
            type: string
            description: "User who owns this session"
            required: true
          - id: messages
            type: list<message>
            description: "Ordered conversation history"
            required: true
          - id: created-at
            type: datetime
            description: "Session creation timestamp"
            required: true
          - id: updated-at
            type: datetime
            description: "Last update timestamp"
            required: true
          - id: status
            type: enum{active, paused, completed, expired}
            description: "Current session state"
            required: true
            default: active
          - id: metadata
            type: map<string, string>
            description: "Additional session metadata"
            required: false
    
    - id: message
      kind: struct
      purpose: "Single message in conversation"
      owners: [user-session-service]
      schema:
        fields:
          - id: message-id
            type: string
            description: "Unique message identifier"
            required: true
          - id: role
            type: enum{user, assistant, system, tool}
            description: "Message sender role"
            required: true
          - id: content
            type: string
            description: "Message text content"
            required: true
          - id: timestamp
            type: datetime
            description: "Message creation time"
            required: true
          - id: tool-calls
            type: list<tool-call>
            description: "Tool invocations in this message"
            required: false
    
    - id: session-summary
      kind: struct
      purpose: "Aggregated session statistics for reporting"
      owners: [user-session-service]
      schema:
        fields:
          - id: session-id
            type: string
            required: true
          - id: total-messages
            type: integer
            required: true
          - id: duration-seconds
            type: integer
            required: true
          - id: user-message-count
            type: integer
            required: true
          - id: assistant-message-count
            type: integer
            required: true
  
  access:
    rules:
      - id: read-own-session
        target: user-session
        read: user-id-match
        write: user-session-service
        visibility: private
        constraints: [user-authenticated, session-owner-match]
        auth: [bearer-token]
        description: "Users can read only their own sessions"
      
      - id: service-read-write
        target: user-session
        read: user-session-service|checkpoint-service|analytics-service
        write: user-session-service
        visibility: internal
        description: "Internal services can read sessions; only session service writes"
      
      - id: read-summary
        target: session-summary
        read: analytics-service|admin-dashboard
        visibility: internal
        description: "Analytics and admin can read aggregated summaries"
  
  manipulation:
    mutations:
      - id: create-session
        target: user-session
        kind: create
        operation: "create_session(user_id: string) -> session-id"
        preconditions:
          - user-authenticated
          - user-has-no-active-session
        postconditions:
          - session-exists
          - session-status-active
          - initial-system-message-added
        side_effects:
          - mv-notify-session-created
          - mv-checkpoint-initial-state
        validation:
          - user-id-valid-format
        description: "Create new session for authenticated user"
      
      - id: append-message
        target: user-session
        kind: update
        operation: "append_message(session_id: string, message: message)"
        preconditions:
          - session-exists
          - session-active
          - message-valid-schema
          - user-owns-session
        postconditions:
          - message-in-history
          - session-updated-timestamp-current
          - message-count-incremented
        side_effects:
          - mv-trigger-checkpoint
        validation:
          - message-schema-validation
          - content-not-empty
        description: "Append validated message to session history"
      
      - id: update-session-status
        target: user-session
        kind: update
        operation: "update_status(session_id: string, status: enum)"
        preconditions:
          - session-exists
          - status-transition-valid
        postconditions:
          - session-status-updated
          - session-updated-timestamp-current
        side_effects:
          - mv-status-change-notification
        description: "Update session status with valid state transition"
      
      - id: expire-session
        target: user-session
        kind: update
        operation: "expire_session(session_id: string)"
        preconditions:
          - session-exists
          - session-inactive-timeout-exceeded
        postconditions:
          - session-status-expired
        side_effects:
          - mv-cleanup-resources
          - mv-archive-to-cold-storage
        description: "Expire inactive sessions and trigger cleanup"
  
  extract:
    transforms:
      - id: generate-session-summary
        inputs: [user-session]
        output: session-summary
        method: aggregation
        deterministic: true
        cacheable: true
        description: "Generate statistical summary from session data"
        algorithm: |
          1. Count total messages
          2. Calculate session duration (created_at to updated_at)
          3. Count messages by role (user, assistant, system, tool)
          4. Return aggregated summary structure
      
      - id: extract-recent-context
        inputs: [user-session]
        output: context-window
        method: projection
        deterministic: true
        cacheable: false
        description: "Extract last N messages for context window"
        algorithm: |
          1. Get last N messages from history
          2. Filter by relevance criteria
          3. Format for context consumption
      
      - id: classify-session-intent
        inputs: [user-session]
        output: intent-classification
        method: inference
        deterministic: false
        cacheable: false
        description: "Classify primary user intent from conversation"
        algorithm: |
          1. Extract user messages only
          2. Apply NLP classification model
          3. Return intent category with confidence score
  
  movement:
    routes:
      - id: mv-receive-user-message
        direction: in
        from: external
        to: user-session-service
        medium: network
        payload: message
        protocol: https
        trigger: request
        reliability: exactly-once
        async: false
        latency: 10ms
        description: "Receive user messages via HTTPS API"
      
      - id: mv-checkpoint-session
        direction: out
        from: user-session-service
        to: checkpoint-service
        medium: network
        payload: user-session
        protocol: grpc
        trigger: event
        reliability: at-least-once
        async: true
        latency: 50ms
        description: "Persist session state to checkpoint service"
      
      - id: mv-notify-session-created
        direction: out
        from: user-session-service
        to: notification-service
        medium: process
        payload: session-event
        protocol: json
        trigger: mutation
        reliability: best-effort
        async: true
        description: "Notify services of new session creation"
      
      - id: mv-publish-metrics
        direction: out
        from: user-session-service
        to: metrics-service
        medium: network
        payload: session-metrics
        protocol: https
        trigger: schedule
        reliability: best-effort
        async: true
        latency: 100ms
        description: "Publish session metrics periodically"
      
      - id: mv-archive-session
        direction: out
        from: user-session-service
        to: archive-storage
        medium: fs
        payload: user-session
        protocol: binary
        trigger: mutation
        reliability: at-least-once
        async: true
        description: "Archive expired sessions to cold storage"
  
  coordination:
    primitives:
      - id: session-lifecycle-orchestration
        kind: orchestration
        participants:
          - user-session-service
          - checkpoint-service
          - notification-service
        drives:
          - mv-checkpoint-session
          - mv-notify-session-created
        guarantees:
          - session-state-consistency
          - ordered-message-processing
          - exactly-once-session-creation
        failure_modes:
          - checkpoint-service-unavailable
          - notification-delivery-failed
          - network-partition
        description: "Orchestrate complete session lifecycle from creation through expiration"
      
      - id: periodic-checkpoint
        kind: scheduling
        participants:
          - user-session-service
          - checkpoint-service
        drives:
          - mv-checkpoint-session
        guarantees:
          - checkpoint-every-5-minutes
          - no-data-loss-beyond-5-minutes
        description: "Schedule automatic session checkpointing every 5 minutes"
      
      - id: session-modification-lock
        kind: locking
        participants:
          - user-session-service
        drives: []
        guarantees:
          - no-concurrent-modification
          - serialized-message-append
        failure_modes:
          - lock-timeout-5-seconds
          - deadlock-detection
        description: "Prevent concurrent modifications to same session"
      
      - id: checkpoint-retry-policy
        kind: retry
        participants:
          - user-session-service
          - checkpoint-service
        drives:
          - mv-checkpoint-session
        guarantees:
          - exponential-backoff
          - max-3-retries
          - eventual-persistence
        failure_modes:
          - all-retries-exhausted
        description: "Retry failed checkpoint writes with exponential backoff"
      
      - id: session-expiration-cleanup
        kind: scheduling
        participants:
          - user-session-service
        drives:
          - mv-archive-session
        guarantees:
          - daily-cleanup-scan
          - expired-sessions-archived
        description: "Daily scan to expire and archive inactive sessions"

dependencies:
  runtime:
    - checkpoint-service
    - notification-service
    - metrics-service
  build: []
  feature_flags:
    - session-persistence
    - session-analytics
  external:
    - serde
    - tokio
    - ulid

risks:
  - id: session-data-loss
    category: reliability
    impact: high
    likelihood: low
    description: "Session data could be lost if checkpoint service is unavailable"
    mitigation: "Multiple checkpoint retries, local buffer, dead letter queue"
    status: mitigated
  
  - id: concurrent-modification
    category: reliability
    impact: medium
    likelihood: medium
    description: "Race conditions during concurrent session modifications"
    mitigation: "Distributed locking, optimistic concurrency control"
    status: mitigated
  
  - id: session-data-exposure
    category: security
    impact: critical
    likelihood: low
    description: "Unauthorized access to user conversation data"
    mitigation: "Strong authentication, encryption at rest and in transit, access logging"
    status: mitigated
  
  - id: checkpoint-service-overload
    category: performance
    impact: medium
    likelihood: medium
    description: "High volume of checkpoint writes could overload service"
    mitigation: "Rate limiting, batching, backpressure handling"
    status: mitigated

integration:
  implements:
    - session-manager-interface
    - state-provider-interface
  extends: []
  replaces:
    - legacy-session-handler

examples:
  - name: "Create and use session"
    description: "Complete session lifecycle from creation to message append"
    code: |
      // Create new session
      let session_id = session_service.create_session("user-123");
      
      // Append user message
      let message = Message {
        message_id: generate_ulid(),
        role: Role::User,
        content: "Hello, how can you help me?",
        timestamp: now(),
        tool_calls: None
      };
      session_service.append_message(session_id, message);
      
      // Append assistant response
      let response = Message {
        message_id: generate_ulid(),
        role: Role::Assistant,
        content: "I'm here to assist you!",
        timestamp: now(),
        tool_calls: None
      };
      session_service.append_message(session_id, response);
      
      // Generate summary
      let summary = session_service.generate_summary(session_id);
      println!("Session has {} messages", summary.total_messages);
  
  - name: "Handle session expiration"
    description: "Check and expire inactive sessions"
    code: |
      // Daily cleanup task
      let inactive_sessions = session_service.find_inactive_sessions(
        timeout_threshold: Duration::hours(24)
      );
      
      for session_id in inactive_sessions {
        session_service.expire_session(session_id);
        // Triggers mv-archive-session and mv-cleanup-resources
      }
```

## Best Practices

### 1. Start with Information Domain

Always begin by defining your data structures. The other five domains describe operations on this data.

### 2. Use Descriptive IDs

Use kebab-case for IDs and make them descriptive: `user-session` not `us`, `append-message` not `append`.

### 3. Document Purpose and Description

Every entity, rule, mutation, transform, route, and primitive should have clear `purpose` or `description` fields.

### 4. Link Domains Through IDs

Use consistent IDs across domains:
- `information.id` referenced in `access.target`
- `manipulation.id` referenced in `movement.side_effects`
- `movement.id` referenced in `coordination.drives`

### 5. Specify Preconditions and Postconditions

For mutations, clearly state what must be true before and after the operation.

### 6. Include Risk Assessment

Document risks, especially for security and reliability concerns.

### 7. Provide Examples

Include realistic usage examples to help readers understand the component.

### 8. Use Type References

Reference other components using the format `component:entity-id` (e.g., `auth-service:user-profile`).

### 9. Be Explicit About Reliability

In movement routes, always specify reliability guarantees and failure modes.

### 10. Keep Dependencies Current

Maintain accurate dependency lists for runtime, build, and external requirements.

## Naming Conventions

- **Component IDs:** kebab-case (`user-session-service`, `checkpoint-service`)
- **Entity IDs:** kebab-case (`user-session`, `message`, `session-summary`)
- **Field names:** kebab-case (`session-id`, `created-at`, `message-count`)
- **File names:** `component-name.udml.yaml`

## Field Requirements

### Required Fields

- `id` - Component identifier
- `layer` - Architectural layer (runtime|infrastructure|boundary|support|plugin)
- `ownership` - Ownership model (crate|wasm|external)
- `summary` - Brief description
- At least one domain in `provides`

### Recommended Fields

- `metadata.version` - Semantic version
- `dependencies` - Component dependencies
- `risks` - Risk assessment

### Optional Fields

- All other metadata
- `integration` section
- `examples` section

## Validation Checklist

- [ ] Component `id` is unique and kebab-case
- [ ] `layer` is one of: runtime, infrastructure, boundary, support, plugin
- [ ] `ownership` is one of: crate, wasm, external
- [ ] At least one domain has content
- [ ] All `information` entities have unique `id`
- [ ] All `access` rules reference valid `information` entities
- [ ] All `manipulation` mutations reference valid `information` entities
- [ ] All `extract` transforms reference valid inputs/outputs
- [ ] All `movement` routes have valid `from`/`to` components
- [ ] All `coordination` primitives reference valid participants
- [ ] Cross-references are consistent (e.g., `side_effects` reference real routes)
- [ ] Risk assessment includes mitigation strategies

## Common Patterns

### Pattern: CRUD Service

```yaml
information:
  - id: entity
    kind: struct
    # ... fields

manipulation:
  mutations:
    - id: create-entity
      kind: create
    - id: read-entity
      kind: read  # Usually in access domain
    - id: update-entity
      kind: update
    - id: delete-entity
      kind: delete
```

### Pattern: Data Pipeline

```yaml
information:
  - id: raw-data
  - id: processed-data

extract:
  transforms:
    - id: process-data
      inputs: [raw-data]
      output: processed-data

movement:
  routes:
    - id: ingest-raw
      to: pipeline-service
      payload: raw-data
    - id: emit-processed
      from: pipeline-service
      payload: processed-data
```

### Pattern: Event-Driven Service

```yaml
movement:
  routes:
    - id: receive-event
      direction: in
      trigger: event
    - id: emit-event
      direction: out
      trigger: mutation

coordination:
  primitives:
    - id: event-orchestration
      kind: orchestration
      drives: [receive-event, emit-event]
```

### Pattern: Stateful Service with Persistence

```yaml
information:
  - id: state
manipulation:
  mutations:
    - id: update-state
      side_effects: [persist-state]
movement:
  routes:
    - id: persist-state
      to: storage-service
      trigger: mutation
coordination:
  primitives:
    - id: auto-checkpoint
      kind: scheduling
      drives: [persist-state]
```

## Conclusion

UDML provides a comprehensive, standardized way to describe software components through data and its transformations. By consistently applying the six domains—Information, Access, Manipulation, Extract, Movement, and Coordination—you create machine-readable specifications that can drive code generation, validation, analysis, and documentation.

The key to successful UDML adoption is:
1. Think data-first, not behavior-first
2. Be explicit about all operations and constraints
3. Link domains through consistent ID references
4. Document risks and dependencies
5. Provide clear examples

With UDML, architecture becomes data calculus, and systems become compositions of well-defined data morphisms.
