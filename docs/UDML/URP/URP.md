# UDML Runtime Packet (URP)

## Abstract

The **UDML Runtime Packet (URP)** is a standardized runtime representation of UDML components. While UDML defines the design-time specification of software systems, URP provides the runtime execution model—a universal message envelope that modules exchange during execution.

URP ensures that runtime behaviors directly align with static UDML specifications, creating a complete lifecycle from design-time → runtime.

## Core Principle

> **A URP is a universal, language-agnostic structure that contains the six UDML domains and represents a live, runtime realization of UDML-defined information and rules.**

Each URP instance carries:
1. The **data payload** (Information domain)
2. **Access control context** (Access domain)
3. **Operation being performed** (Manipulation domain)
4. **Transformation intent** (Extract domain)
5. **Routing metadata** (Movement domain)
6. **Orchestration state** (Coordination domain)

## Design Goals

1. **Universal Interoperability**: Single message format for all module communication
2. **Design-Runtime Alignment**: Direct mapping to UDML specifications
3. **Language Agnostic**: Works across Rust, Python, JavaScript, etc.
4. **Validation Ready**: Schema-driven validation before transmission
5. **Traceable**: Every URP can be traced back to UDML definitions
6. **Extensible**: Support for custom metadata without breaking compatibility

## URP Structure

### Top-Level Envelope

```json
{
  "$schema": "https://udml.podtan.com/urp/v0.1/schema.json",
  "version": "0.1",
  "urp_id": "01HQZXC7JKTFV8RQWX3K9M2N4P",
  "timestamp": "2025-11-11T10:30:00Z",
  "source_component": "user-session-service",
  "target_component": "checkpoint-service",
  "trace_id": "trace-01HQZXC7JKTFV8RQWX3K9M2N4P",
  "correlation_id": "corr-session-123",
  
  "information": { },
  "access": { },
  "manipulation": { },
  "extract": { },
  "movement": { },
  "coordination": { }
}
```

### Field Descriptions

- **`$schema`**: URI to URP JSON Schema for validation
- **`version`**: URP specification version (semantic versioning)
- **`urp_id`**: Unique identifier for this packet (ULID recommended)
- **`timestamp`**: ISO 8601 timestamp when packet was created
- **`source_component`**: UDML component ID that created this packet
- **`target_component`**: UDML component ID that should receive this packet
- **`trace_id`**: Distributed tracing ID for correlation
- **`correlation_id`**: Application-level correlation identifier

## The Six URP Domains

### 1. Information Domain (Runtime Data)

Contains the actual data payload being transmitted, referencing UDML Information entities.

```json
"information": {
  "entity_id": "user-session",
  "entity_type": "struct",
  "data": {
    "session_id": "session-01HQZXC7JKTFV8RQWX3K9M2N4P",
    "user_id": "user-123",
    "messages": [
      {
        "message_id": "msg-001",
        "role": "user",
        "content": "Hello, how can you help me?",
        "timestamp": "2025-11-11T10:29:45Z"
      }
    ],
    "created_at": "2025-11-11T10:29:00Z",
    "updated_at": "2025-11-11T10:29:45Z",
    "status": "active"
  },
  "schema_ref": "user-session-service#user-session",
  "version": "1.0.0"
}
```

**Fields:**
- **`entity_id`**: Reference to UDML Information entity ID
- **`entity_type`**: Type of entity (struct, enum, blob, stream, etc.)
- **`data`**: Actual runtime data payload
- **`schema_ref`**: Full reference to UDML schema (`component#entity`)
- **`version`**: Data schema version

### 2. Access Domain (Runtime Authorization)

Contains access control context for this runtime operation.

```json
"access": {
  "rule_id": "service-read-write",
  "principal": {
    "type": "service",
    "id": "user-session-service",
    "roles": ["session-manager", "state-writer"]
  },
  "auth_method": "bearer-token",
  "auth_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
  "visibility": "internal",
  "constraints_satisfied": [
    "session-exists",
    "service-authenticated"
  ],
  "permissions": {
    "read": true,
    "write": true,
    "delete": false
  }
}
```

**Fields:**
- **`rule_id`**: Reference to UDML Access rule being applied
- **`principal`**: Identity performing the operation (user, service, system)
- **`auth_method`**: Authentication method used
- **`auth_token`**: Optional authentication token (handle securely)
- **`visibility`**: Data visibility level (public, internal, private)
- **`constraints_satisfied`**: List of constraint IDs that are satisfied
- **`permissions`**: Explicit permissions for this operation

### 3. Manipulation Domain (Runtime Operation)

Describes the mutation operation being performed.

```json
"manipulation": {
  "mutation_id": "append-message",
  "operation": "append_message",
  "kind": "update",
  "parameters": {
    "session_id": "session-01HQZXC7JKTFV8RQWX3K9M2N4P",
    "message": {
      "message_id": "msg-001",
      "role": "user",
      "content": "Hello, how can you help me?",
      "timestamp": "2025-11-11T10:29:45Z"
    }
  },
  "preconditions_checked": [
    "session-exists",
    "session-active",
    "message-valid-schema"
  ],
  "postconditions_expected": [
    "message-in-history",
    "session-updated-timestamp-current"
  ],
  "validation_passed": [
    "message-schema-validation",
    "content-not-empty"
  ],
  "idempotency_key": "idem-msg-001"
}
```

**Fields:**
- **`mutation_id`**: Reference to UDML Manipulation mutation ID
- **`operation`**: Operation name being invoked
- **`kind`**: Mutation kind (create, update, delete, invalidate, correct)
- **`parameters`**: Operation parameters as key-value pairs
- **`preconditions_checked`**: List of precondition IDs that were verified
- **`postconditions_expected`**: List of postcondition IDs that should result
- **`validation_passed`**: List of validation rule IDs that passed
- **`idempotency_key`**: Optional key for idempotent operations

### 4. Extract Domain (Runtime Transformation)

Describes data transformation or derivation happening at runtime.

```json
"extract": {
  "transform_id": "generate-session-summary",
  "method": "aggregation",
  "inputs": [
    {
      "entity_id": "user-session",
      "source": "user-session-service#user-session",
      "data_ref": "information.data"
    }
  ],
  "output": {
    "entity_id": "session-summary",
    "schema_ref": "user-session-service#session-summary"
  },
  "deterministic": true,
  "cacheable": true,
  "cache_key": "summary-session-01HQZXC7JKTFV8RQWX3K9M2N4P",
  "cache_ttl": 300,
  "algorithm_version": "1.0"
}
```

**Fields:**
- **`transform_id`**: Reference to UDML Extract transform ID
- **`method`**: Transformation method (algorithm, template, aggregation, projection, inference)
- **`inputs`**: List of input data sources with references
- **`output`**: Expected output entity specification
- **`deterministic`**: Whether transformation is deterministic
- **`cacheable`**: Whether result can be cached
- **`cache_key`**: Optional cache key for result storage
- **`cache_ttl`**: Cache time-to-live in seconds
- **`algorithm_version`**: Version of transformation algorithm

### 5. Movement Domain (Runtime Routing)

Contains routing and delivery metadata for this packet.

```json
"movement": {
  "route_id": "mv-checkpoint-session",
  "direction": "out",
  "medium": "network",
  "protocol": "grpc",
  "endpoint": "checkpoint-service.internal:9090",
  "reliability": "at-least-once",
  "async": true,
  "trigger": "event",
  "trigger_event": "message-appended",
  "priority": "normal",
  "timeout_ms": 5000,
  "retry_policy": {
    "max_retries": 3,
    "backoff": "exponential",
    "initial_delay_ms": 100
  },
  "compression": "gzip",
  "encryption": "tls-1.3"
}
```

**Fields:**
- **`route_id`**: Reference to UDML Movement route ID
- **`direction`**: Data flow direction (in, out, bi)
- **`medium`**: Communication medium (network, memory, fs, wasm-call, etc.)
- **`protocol`**: Wire protocol (grpc, https, json, binary, etc.)
- **`endpoint`**: Network endpoint or resource locator
- **`reliability`**: Delivery guarantee (best-effort, at-least-once, exactly-once)
- **`async`**: Whether transmission is asynchronous
- **`trigger`**: What triggered this movement (event, schedule, mutation, request)
- **`trigger_event`**: Specific event that triggered transmission
- **`priority`**: Message priority (low, normal, high, critical)
- **`timeout_ms`**: Operation timeout in milliseconds
- **`retry_policy`**: Retry configuration
- **`compression`**: Compression algorithm used
- **`encryption`**: Encryption protocol

### 6. Coordination Domain (Runtime Orchestration)

Contains orchestration and coordination state for this operation.

```json
"coordination": {
  "primitive_id": "session-lifecycle-orchestration",
  "kind": "orchestration",
  "workflow_id": "workflow-01HQZXC7JKTFV8RQWX3K9M2N4P",
  "step": "checkpoint-session-state",
  "step_index": 2,
  "total_steps": 5,
  "status": "in-progress",
  "participants": [
    "user-session-service",
    "checkpoint-service",
    "notification-service"
  ],
  "locks_held": [
    "session-modification-lock"
  ],
  "dependencies": [
    {
      "step": "validate-session",
      "status": "completed"
    },
    {
      "step": "append-message",
      "status": "completed"
    }
  ],
  "next_steps": [
    "notify-session-updated",
    "update-metrics"
  ],
  "failure_mode": null,
  "retry_count": 0,
  "deadline": "2025-11-11T10:31:00Z"
}
```

**Fields:**
- **`primitive_id`**: Reference to UDML Coordination primitive ID
- **`kind`**: Coordination kind (orchestration, scheduling, locking, retry, etc.)
- **`workflow_id`**: Unique workflow instance identifier
- **`step`**: Current step in workflow/coordination
- **`step_index`**: Numeric index of current step
- **`total_steps`**: Total number of steps in workflow
- **`status`**: Coordination status (pending, in-progress, completed, failed)
- **`participants`**: List of participating component IDs
- **`locks_held`**: List of lock IDs currently held
- **`dependencies`**: Steps that must complete before proceeding
- **`next_steps`**: Upcoming steps after current
- **`failure_mode`**: Current failure mode if any
- **`retry_count`**: Number of retries attempted
- **`deadline`**: ISO 8601 deadline for completion

## Complete URP Examples

### Example 1: Session Checkpoint Request

```json
{
  "$schema": "https://udml.podtan.com/urp/v0.1/schema.json",
  "version": "0.1",
  "urp_id": "01HQZXC7JKTFV8RQWX3K9M2N4P",
  "timestamp": "2025-11-11T10:30:00Z",
  "source_component": "user-session-service",
  "target_component": "checkpoint-service",
  "trace_id": "trace-01HQZXC7JKTFV8RQWX3K9M2N4P",
  "correlation_id": "session-01HQZXC7JKTFV8RQWX3K9M2N4P",
  
  "information": {
    "entity_id": "user-session",
    "entity_type": "struct",
    "data": {
      "session_id": "session-01HQZXC7JKTFV8RQWX3K9M2N4P",
      "user_id": "user-123",
      "messages": [
        {
          "message_id": "msg-001",
          "role": "user",
          "content": "Hello, how can you help me?",
          "timestamp": "2025-11-11T10:29:45Z"
        },
        {
          "message_id": "msg-002",
          "role": "assistant",
          "content": "I'm here to assist you with your questions!",
          "timestamp": "2025-11-11T10:29:50Z"
        }
      ],
      "created_at": "2025-11-11T10:29:00Z",
      "updated_at": "2025-11-11T10:29:50Z",
      "status": "active",
      "metadata": {
        "channel": "web",
        "locale": "en-US"
      }
    },
    "schema_ref": "user-session-service#user-session",
    "version": "1.0.0"
  },
  
  "access": {
    "rule_id": "service-read-write",
    "principal": {
      "type": "service",
      "id": "user-session-service",
      "roles": ["session-manager", "state-writer"]
    },
    "auth_method": "mutual-tls",
    "visibility": "internal",
    "constraints_satisfied": [
      "session-exists",
      "session-active",
      "service-authenticated"
    ],
    "permissions": {
      "read": true,
      "write": true,
      "delete": false
    }
  },
  
  "manipulation": {
    "mutation_id": "checkpoint-session",
    "operation": "persist_session",
    "kind": "update",
    "parameters": {
      "session_id": "session-01HQZXC7JKTFV8RQWX3K9M2N4P",
      "checkpoint_type": "incremental"
    },
    "preconditions_checked": [
      "session-exists",
      "session-active",
      "checkpoint-service-available"
    ],
    "postconditions_expected": [
      "session-persisted",
      "checkpoint-timestamp-updated"
    ],
    "validation_passed": [
      "session-data-complete",
      "session-schema-valid"
    ],
    "idempotency_key": "checkpoint-session-01HQZXC7JKTFV8RQWX3K9M2N4P-1731324600"
  },
  
  "extract": {
    "transform_id": null,
    "method": null,
    "inputs": [],
    "output": null,
    "deterministic": true,
    "cacheable": false
  },
  
  "movement": {
    "route_id": "mv-checkpoint-session",
    "direction": "out",
    "medium": "network",
    "protocol": "grpc",
    "endpoint": "checkpoint-service.internal:9090",
    "reliability": "at-least-once",
    "async": true,
    "trigger": "event",
    "trigger_event": "message-appended",
    "priority": "normal",
    "timeout_ms": 5000,
    "retry_policy": {
      "max_retries": 3,
      "backoff": "exponential",
      "initial_delay_ms": 100,
      "max_delay_ms": 1000
    },
    "compression": "gzip",
    "encryption": "tls-1.3"
  },
  
  "coordination": {
    "primitive_id": "session-lifecycle-orchestration",
    "kind": "orchestration",
    "workflow_id": "workflow-01HQZXC7JKTFV8RQWX3K9M2N4P",
    "step": "checkpoint-session-state",
    "step_index": 2,
    "total_steps": 5,
    "status": "in-progress",
    "participants": [
      "user-session-service",
      "checkpoint-service",
      "notification-service"
    ],
    "locks_held": [],
    "dependencies": [
      {
        "step": "validate-session",
        "status": "completed"
      },
      {
        "step": "append-message",
        "status": "completed"
      }
    ],
    "next_steps": [
      "notify-session-updated",
      "update-metrics"
    ],
    "failure_mode": null,
    "retry_count": 0,
    "deadline": "2025-11-11T10:31:00Z"
  }
}
```

### Example 2: Data Transformation Request

```json
{
  "$schema": "https://udml.podtan.com/urp/v0.1/schema.json",
  "version": "0.1",
  "urp_id": "01HQZXD8KLMGW9SRXY4L0N3O5Q",
  "timestamp": "2025-11-11T10:35:00Z",
  "source_component": "analytics-service",
  "target_component": "user-session-service",
  "trace_id": "trace-01HQZXD8KLMGW9SRXY4L0N3O5Q",
  "correlation_id": "analytics-report-daily-001",
  
  "information": {
    "entity_id": "session-summary",
    "entity_type": "struct",
    "data": null,
    "schema_ref": "user-session-service#session-summary",
    "version": "1.0.0"
  },
  
  "access": {
    "rule_id": "read-summary",
    "principal": {
      "type": "service",
      "id": "analytics-service",
      "roles": ["data-reader", "reporter"]
    },
    "auth_method": "api-key",
    "visibility": "internal",
    "constraints_satisfied": [
      "service-authenticated",
      "read-only-access"
    ],
    "permissions": {
      "read": true,
      "write": false,
      "delete": false
    }
  },
  
  "manipulation": {
    "mutation_id": null,
    "operation": "read_summary",
    "kind": null,
    "parameters": {
      "session_id": "session-01HQZXC7JKTFV8RQWX3K9M2N4P"
    },
    "preconditions_checked": [
      "session-exists"
    ],
    "postconditions_expected": [],
    "validation_passed": [
      "session-id-valid-format"
    ],
    "idempotency_key": null
  },
  
  "extract": {
    "transform_id": "generate-session-summary",
    "method": "aggregation",
    "inputs": [
      {
        "entity_id": "user-session",
        "source": "user-session-service#user-session",
        "data_ref": "session-01HQZXC7JKTFV8RQWX3K9M2N4P"
      }
    ],
    "output": {
      "entity_id": "session-summary",
      "schema_ref": "user-session-service#session-summary"
    },
    "deterministic": true,
    "cacheable": true,
    "cache_key": "summary-session-01HQZXC7JKTFV8RQWX3K9M2N4P",
    "cache_ttl": 300,
    "algorithm_version": "1.0"
  },
  
  "movement": {
    "route_id": "mv-request-summary",
    "direction": "in",
    "medium": "network",
    "protocol": "grpc",
    "endpoint": "user-session-service.internal:9091",
    "reliability": "exactly-once",
    "async": false,
    "trigger": "request",
    "trigger_event": null,
    "priority": "normal",
    "timeout_ms": 2000,
    "retry_policy": {
      "max_retries": 2,
      "backoff": "linear",
      "initial_delay_ms": 500,
      "max_delay_ms": 1000
    },
    "compression": null,
    "encryption": "tls-1.3"
  },
  
  "coordination": {
    "primitive_id": "read-orchestration",
    "kind": "orchestration",
    "workflow_id": "workflow-analytics-daily-001",
    "step": "fetch-session-summary",
    "step_index": 5,
    "total_steps": 20,
    "status": "in-progress",
    "participants": [
      "analytics-service",
      "user-session-service"
    ],
    "locks_held": [],
    "dependencies": [
      {
        "step": "identify-sessions",
        "status": "completed"
      }
    ],
    "next_steps": [
      "aggregate-summaries",
      "generate-report"
    ],
    "failure_mode": null,
    "retry_count": 0,
    "deadline": "2025-11-11T10:40:00Z"
  }
}
```

### Example 3: Event Notification

```json
{
  "$schema": "https://udml.podtan.com/urp/v0.1/schema.json",
  "version": "0.1",
  "urp_id": "01HQZXE9LMNH0ATYZ5M1P4R6S",
  "timestamp": "2025-11-11T10:40:00Z",
  "source_component": "user-session-service",
  "target_component": "notification-service",
  "trace_id": "trace-01HQZXE9LMNH0ATYZ5M1P4R6S",
  "correlation_id": "session-01HQZXC7JKTFV8RQWX3K9M2N4P",
  
  "information": {
    "entity_id": "session-event",
    "entity_type": "struct",
    "data": {
      "event_type": "session-created",
      "session_id": "session-01HQZXC7JKTFV8RQWX3K9M2N4P",
      "user_id": "user-123",
      "timestamp": "2025-11-11T10:29:00Z"
    },
    "schema_ref": "user-session-service#session-event",
    "version": "1.0.0"
  },
  
  "access": {
    "rule_id": "broadcast-event",
    "principal": {
      "type": "service",
      "id": "user-session-service",
      "roles": ["event-publisher"]
    },
    "auth_method": "mutual-tls",
    "visibility": "internal",
    "constraints_satisfied": [
      "service-authenticated",
      "event-valid"
    ],
    "permissions": {
      "read": true,
      "write": false,
      "delete": false
    }
  },
  
  "manipulation": {
    "mutation_id": "publish-event",
    "operation": "notify_session_created",
    "kind": "create",
    "parameters": {
      "event_type": "session-created",
      "session_id": "session-01HQZXC7JKTFV8RQWX3K9M2N4P"
    },
    "preconditions_checked": [
      "session-created",
      "event-schema-valid"
    ],
    "postconditions_expected": [
      "event-delivered"
    ],
    "validation_passed": [
      "event-type-valid",
      "payload-complete"
    ],
    "idempotency_key": "event-session-created-01HQZXC7JKTFV8RQWX3K9M2N4P"
  },
  
  "extract": {
    "transform_id": null,
    "method": null,
    "inputs": [],
    "output": null,
    "deterministic": true,
    "cacheable": false
  },
  
  "movement": {
    "route_id": "mv-notify-session-created",
    "direction": "out",
    "medium": "process",
    "protocol": "json",
    "endpoint": "notification-service.internal/events",
    "reliability": "best-effort",
    "async": true,
    "trigger": "mutation",
    "trigger_event": "session-created",
    "priority": "low",
    "timeout_ms": 1000,
    "retry_policy": {
      "max_retries": 1,
      "backoff": "linear",
      "initial_delay_ms": 200,
      "max_delay_ms": 200
    },
    "compression": null,
    "encryption": null
  },
  
  "coordination": {
    "primitive_id": "session-lifecycle-orchestration",
    "kind": "orchestration",
    "workflow_id": "workflow-01HQZXC7JKTFV8RQWX3K9M2N4P",
    "step": "notify-session-created",
    "step_index": 1,
    "total_steps": 5,
    "status": "in-progress",
    "participants": [
      "user-session-service",
      "notification-service"
    ],
    "locks_held": [],
    "dependencies": [
      {
        "step": "create-session",
        "status": "completed"
      }
    ],
    "next_steps": [
      "checkpoint-session-state"
    ],
    "failure_mode": null,
    "retry_count": 0,
    "deadline": "2025-11-11T10:41:00Z"
  }
}
```

## URP JSON Schema

The following JSON Schema defines the normative structure for URP packets:

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "$id": "https://udml.podtan.com/urp/v0.1/schema.json",
  "title": "UDML Runtime Packet (URP)",
  "description": "Universal runtime message format for UDML-based systems",
  "type": "object",
  "required": [
    "$schema",
    "version",
    "urp_id",
    "timestamp",
    "source_component",
    "target_component",
    "information",
    "access",
    "manipulation",
    "extract",
    "movement",
    "coordination"
  ],
  "properties": {
    "$schema": {
      "type": "string",
      "format": "uri",
      "const": "https://udml.podtan.com/urp/v0.1/schema.json",
      "description": "URI to URP JSON Schema for validation"
    },
    "version": {
      "type": "string",
      "pattern": "^\\d+\\.\\d+$",
      "description": "URP specification version (semantic versioning)"
    },
    "urp_id": {
      "type": "string",
      "pattern": "^[0-9A-HJKMNP-TV-Z]{26}$",
      "description": "Unique identifier for this packet (ULID format)"
    },
    "timestamp": {
      "type": "string",
      "format": "date-time",
      "description": "ISO 8601 timestamp when packet was created"
    },
    "source_component": {
      "type": "string",
      "pattern": "^[a-z][a-z0-9-]*$",
      "description": "UDML component ID that created this packet"
    },
    "target_component": {
      "type": "string",
      "pattern": "^[a-z][a-z0-9-]*$",
      "description": "UDML component ID that should receive this packet"
    },
    "trace_id": {
      "type": "string",
      "description": "Distributed tracing ID for correlation"
    },
    "correlation_id": {
      "type": "string",
      "description": "Application-level correlation identifier"
    },
    "information": {
      "type": "object",
      "description": "Information domain: runtime data payload",
      "required": ["entity_id", "entity_type", "schema_ref"],
      "properties": {
        "entity_id": {
          "type": "string",
          "pattern": "^[a-z][a-z0-9-]*$",
          "description": "Reference to UDML Information entity ID"
        },
        "entity_type": {
          "type": "string",
          "enum": ["struct", "enum", "alias", "collection", "blob", "stream", "schema", "code"],
          "description": "Type of entity"
        },
        "data": {
          "description": "Actual runtime data payload (can be null for requests)"
        },
        "schema_ref": {
          "type": "string",
          "pattern": "^[a-z][a-z0-9-]*#[a-z][a-z0-9-]*$",
          "description": "Full reference to UDML schema (component#entity)"
        },
        "version": {
          "type": "string",
          "pattern": "^\\d+\\.\\d+\\.\\d+$",
          "description": "Data schema version (semantic versioning)"
        }
      }
    },
    "access": {
      "type": "object",
      "description": "Access domain: runtime authorization context",
      "required": ["principal", "visibility", "permissions"],
      "properties": {
        "rule_id": {
          "type": "string",
          "pattern": "^[a-z][a-z0-9-]*$",
          "description": "Reference to UDML Access rule being applied"
        },
        "principal": {
          "type": "object",
          "required": ["type", "id"],
          "properties": {
            "type": {
              "type": "string",
              "enum": ["user", "service", "system", "anonymous"],
              "description": "Type of principal"
            },
            "id": {
              "type": "string",
              "description": "Principal identifier"
            },
            "roles": {
              "type": "array",
              "items": {
                "type": "string"
              },
              "description": "Roles assigned to principal"
            }
          }
        },
        "auth_method": {
          "type": "string",
          "enum": ["bearer-token", "api-key", "oauth2", "mutual-tls", "basic-auth", "none"],
          "description": "Authentication method used"
        },
        "auth_token": {
          "type": "string",
          "description": "Optional authentication token (handle securely)"
        },
        "visibility": {
          "type": "string",
          "enum": ["public", "internal", "private", "restricted"],
          "description": "Data visibility level"
        },
        "constraints_satisfied": {
          "type": "array",
          "items": {
            "type": "string",
            "pattern": "^[a-z][a-z0-9-]*$"
          },
          "description": "List of constraint IDs that are satisfied"
        },
        "permissions": {
          "type": "object",
          "required": ["read", "write", "delete"],
          "properties": {
            "read": {
              "type": "boolean"
            },
            "write": {
              "type": "boolean"
            },
            "delete": {
              "type": "boolean"
            }
          },
          "description": "Explicit permissions for this operation"
        }
      }
    },
    "manipulation": {
      "type": "object",
      "description": "Manipulation domain: runtime operation",
      "required": ["operation"],
      "properties": {
        "mutation_id": {
          "type": ["string", "null"],
          "pattern": "^[a-z][a-z0-9-]*$",
          "description": "Reference to UDML Manipulation mutation ID"
        },
        "operation": {
          "type": "string",
          "description": "Operation name being invoked"
        },
        "kind": {
          "type": ["string", "null"],
          "enum": ["create", "update", "delete", "invalidate", "correct", null],
          "description": "Mutation kind"
        },
        "parameters": {
          "type": "object",
          "description": "Operation parameters as key-value pairs"
        },
        "preconditions_checked": {
          "type": "array",
          "items": {
            "type": "string",
            "pattern": "^[a-z][a-z0-9-]*$"
          },
          "description": "List of precondition IDs that were verified"
        },
        "postconditions_expected": {
          "type": "array",
          "items": {
            "type": "string",
            "pattern": "^[a-z][a-z0-9-]*$"
          },
          "description": "List of postcondition IDs that should result"
        },
        "validation_passed": {
          "type": "array",
          "items": {
            "type": "string",
            "pattern": "^[a-z][a-z0-9-]*$"
          },
          "description": "List of validation rule IDs that passed"
        },
        "idempotency_key": {
          "type": ["string", "null"],
          "description": "Optional key for idempotent operations"
        }
      }
    },
    "extract": {
      "type": "object",
      "description": "Extract domain: runtime transformation",
      "required": ["deterministic", "cacheable"],
      "properties": {
        "transform_id": {
          "type": ["string", "null"],
          "pattern": "^[a-z][a-z0-9-]*$",
          "description": "Reference to UDML Extract transform ID"
        },
        "method": {
          "type": ["string", "null"],
          "enum": ["algorithm", "template", "aggregation", "projection", "inference", null],
          "description": "Transformation method"
        },
        "inputs": {
          "type": "array",
          "items": {
            "type": "object",
            "required": ["entity_id", "source"],
            "properties": {
              "entity_id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]*$"
              },
              "source": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]*#[a-z][a-z0-9-]*$"
              },
              "data_ref": {
                "type": "string"
              }
            }
          },
          "description": "List of input data sources with references"
        },
        "output": {
          "type": ["object", "null"],
          "properties": {
            "entity_id": {
              "type": "string",
              "pattern": "^[a-z][a-z0-9-]*$"
            },
            "schema_ref": {
              "type": "string",
              "pattern": "^[a-z][a-z0-9-]*#[a-z][a-z0-9-]*$"
            }
          },
          "description": "Expected output entity specification"
        },
        "deterministic": {
          "type": "boolean",
          "description": "Whether transformation is deterministic"
        },
        "cacheable": {
          "type": "boolean",
          "description": "Whether result can be cached"
        },
        "cache_key": {
          "type": ["string", "null"],
          "description": "Optional cache key for result storage"
        },
        "cache_ttl": {
          "type": ["integer", "null"],
          "minimum": 0,
          "description": "Cache time-to-live in seconds"
        },
        "algorithm_version": {
          "type": ["string", "null"],
          "description": "Version of transformation algorithm"
        }
      }
    },
    "movement": {
      "type": "object",
      "description": "Movement domain: runtime routing",
      "required": ["direction", "medium", "reliability", "async"],
      "properties": {
        "route_id": {
          "type": ["string", "null"],
          "pattern": "^[a-z][a-z0-9-]*$",
          "description": "Reference to UDML Movement route ID"
        },
        "direction": {
          "type": "string",
          "enum": ["in", "out", "bi"],
          "description": "Data flow direction"
        },
        "medium": {
          "type": "string",
          "enum": ["memory", "wasm-call", "fs", "network", "process", "stdout", "stderr", "ipc"],
          "description": "Communication medium"
        },
        "protocol": {
          "type": ["string", "null"],
          "description": "Wire protocol (grpc, https, json, binary, etc.)"
        },
        "endpoint": {
          "type": ["string", "null"],
          "description": "Network endpoint or resource locator"
        },
        "reliability": {
          "type": "string",
          "enum": ["best-effort", "at-least-once", "exactly-once", "not-applicable"],
          "description": "Delivery guarantee"
        },
        "async": {
          "type": "boolean",
          "description": "Whether transmission is asynchronous"
        },
        "trigger": {
          "type": ["string", "null"],
          "enum": ["event", "schedule", "mutation", "request", null],
          "description": "What triggered this movement"
        },
        "trigger_event": {
          "type": ["string", "null"],
          "description": "Specific event that triggered transmission"
        },
        "priority": {
          "type": ["string", "null"],
          "enum": ["low", "normal", "high", "critical", null],
          "description": "Message priority"
        },
        "timeout_ms": {
          "type": ["integer", "null"],
          "minimum": 0,
          "description": "Operation timeout in milliseconds"
        },
        "retry_policy": {
          "type": ["object", "null"],
          "properties": {
            "max_retries": {
              "type": "integer",
              "minimum": 0
            },
            "backoff": {
              "type": "string",
              "enum": ["linear", "exponential", "constant"]
            },
            "initial_delay_ms": {
              "type": "integer",
              "minimum": 0
            },
            "max_delay_ms": {
              "type": "integer",
              "minimum": 0
            }
          },
          "description": "Retry configuration"
        },
        "compression": {
          "type": ["string", "null"],
          "description": "Compression algorithm used"
        },
        "encryption": {
          "type": ["string", "null"],
          "description": "Encryption protocol"
        }
      }
    },
    "coordination": {
      "type": "object",
      "description": "Coordination domain: runtime orchestration",
      "required": ["kind", "status", "participants"],
      "properties": {
        "primitive_id": {
          "type": ["string", "null"],
          "pattern": "^[a-z][a-z0-9-]*$",
          "description": "Reference to UDML Coordination primitive ID"
        },
        "kind": {
          "type": "string",
          "enum": ["orchestration", "scheduling", "locking", "retry", "checkpoint", "classification", "consensus"],
          "description": "Coordination kind"
        },
        "workflow_id": {
          "type": ["string", "null"],
          "description": "Unique workflow instance identifier"
        },
        "step": {
          "type": ["string", "null"],
          "description": "Current step in workflow/coordination"
        },
        "step_index": {
          "type": ["integer", "null"],
          "minimum": 0,
          "description": "Numeric index of current step"
        },
        "total_steps": {
          "type": ["integer", "null"],
          "minimum": 0,
          "description": "Total number of steps in workflow"
        },
        "status": {
          "type": "string",
          "enum": ["pending", "in-progress", "completed", "failed", "cancelled"],
          "description": "Coordination status"
        },
        "participants": {
          "type": "array",
          "items": {
            "type": "string",
            "pattern": "^[a-z][a-z0-9-]*$"
          },
          "description": "List of participating component IDs"
        },
        "locks_held": {
          "type": "array",
          "items": {
            "type": "string",
            "pattern": "^[a-z][a-z0-9-]*$"
          },
          "description": "List of lock IDs currently held"
        },
        "dependencies": {
          "type": "array",
          "items": {
            "type": "object",
            "required": ["step", "status"],
            "properties": {
              "step": {
                "type": "string"
              },
              "status": {
                "type": "string",
                "enum": ["pending", "in-progress", "completed", "failed"]
              }
            }
          },
          "description": "Steps that must complete before proceeding"
        },
        "next_steps": {
          "type": "array",
          "items": {
            "type": "string"
          },
          "description": "Upcoming steps after current"
        },
        "failure_mode": {
          "type": ["string", "null"],
          "description": "Current failure mode if any"
        },
        "retry_count": {
          "type": "integer",
          "minimum": 0,
          "description": "Number of retries attempted"
        },
        "deadline": {
          "type": ["string", "null"],
          "format": "date-time",
          "description": "ISO 8601 deadline for completion"
        }
      }
    }
  },
  "additionalProperties": false
}
```

## URP Validation

### Validation Rules

1. **Schema Compliance**: All URPs MUST validate against the JSON Schema
2. **Reference Integrity**: All IDs referenced (entity_id, rule_id, etc.) MUST exist in corresponding UDML specifications
3. **Domain Consistency**: Data in `information.data` MUST match the schema defined in UDML
4. **Authorization Validity**: Access rules referenced MUST be satisfied by principal
5. **Precondition Verification**: All preconditions in `manipulation.preconditions_checked` MUST be verified before transmission
6. **Route Compatibility**: Movement configuration MUST match UDML route definition

### Validation Example (Python)

```python
import json
import jsonschema
from datetime import datetime

def validate_urp(urp_data, urp_schema, udml_specs):
    """
    Validate a URP packet against JSON Schema and UDML specifications
    
    Args:
        urp_data: URP packet as dictionary
        urp_schema: URP JSON Schema
        udml_specs: Dictionary of UDML component specifications
    
    Returns:
        Tuple of (is_valid, errors)
    """
    errors = []
    
    # 1. Validate against JSON Schema
    try:
        jsonschema.validate(instance=urp_data, schema=urp_schema)
    except jsonschema.ValidationError as e:
        errors.append(f"Schema validation error: {e.message}")
        return False, errors
    
    # 2. Validate reference integrity
    source_comp = urp_data["source_component"]
    target_comp = urp_data["target_component"]
    
    if source_comp not in udml_specs:
        errors.append(f"Unknown source component: {source_comp}")
    
    if target_comp not in udml_specs:
        errors.append(f"Unknown target component: {target_comp}")
    
    # 3. Validate entity reference
    entity_id = urp_data["information"]["entity_id"]
    schema_ref = urp_data["information"]["schema_ref"]
    
    expected_ref = f"{source_comp}#{entity_id}"
    if schema_ref != expected_ref:
        errors.append(f"Schema reference mismatch: expected {expected_ref}, got {schema_ref}")
    
    # 4. Validate access rule
    if urp_data["access"]["rule_id"]:
        rule_id = urp_data["access"]["rule_id"]
        comp_spec = udml_specs.get(source_comp, {})
        access_rules = comp_spec.get("provides", {}).get("access", {}).get("rules", [])
        
        if not any(rule["id"] == rule_id for rule in access_rules):
            errors.append(f"Unknown access rule: {rule_id}")
    
    # 5. Validate movement route
    if urp_data["movement"]["route_id"]:
        route_id = urp_data["movement"]["route_id"]
        comp_spec = udml_specs.get(source_comp, {})
        routes = comp_spec.get("provides", {}).get("movement", {}).get("routes", [])
        
        if not any(route["id"] == route_id for route in routes):
            errors.append(f"Unknown movement route: {route_id}")
    
    return len(errors) == 0, errors

# Usage
with open("urp_packet.json") as f:
    urp = json.load(f)

with open("urp_schema.json") as f:
    schema = json.load(f)

with open("udml_specs.json") as f:
    specs = json.load(f)

is_valid, errors = validate_urp(urp, schema, specs)
if not is_valid:
    print("Validation errors:", errors)
else:
    print("URP is valid")
```

## URP Implementation Patterns

### Pattern 1: Request-Response

```rust
// Send request URP
let request = URP {
    urp_id: generate_ulid(),
    source_component: "client-service".into(),
    target_component: "user-session-service".into(),
    information: Information {
        entity_id: "user-session".into(),
        data: None, // Request has no data
        // ...
    },
    manipulation: Manipulation {
        operation: "get_session".into(),
        parameters: hashmap!{
            "session_id" => "session-123"
        },
        // ...
    },
    // ...
};

// Receive response URP
let response = URP {
    urp_id: generate_ulid(),
    correlation_id: request.urp_id.clone(),
    source_component: "user-session-service".into(),
    target_component: "client-service".into(),
    information: Information {
        entity_id: "user-session".into(),
        data: Some(session_data),
        // ...
    },
    // ...
};
```

### Pattern 2: Event Publication

```rust
// Publish event URP
let event = URP {
    urp_id: generate_ulid(),
    source_component: "user-session-service".into(),
    target_component: "notification-service".into(),
    information: Information {
        entity_id: "session-event".into(),
        data: Some(json!({
            "event_type": "session-created",
            "session_id": "session-123"
        })),
        // ...
    },
    movement: Movement {
        reliability: "best-effort".into(),
        async: true,
        trigger: Some("mutation".into()),
        // ...
    },
    // ...
};
```

### Pattern 3: Workflow Orchestration

```rust
// Step 1: Validate
let validate_urp = URP {
    coordination: Coordination {
        workflow_id: Some("workflow-123".into()),
        step: Some("validate".into()),
        step_index: Some(1),
        total_steps: Some(3),
        status: "in-progress".into(),
        // ...
    },
    // ...
};

// Step 2: Process
let process_urp = URP {
    coordination: Coordination {
        workflow_id: Some("workflow-123".into()),
        step: Some("process".into()),
        step_index: Some(2),
        total_steps: Some(3),
        dependencies: vec![
            Dependency {
                step: "validate".into(),
                status: "completed".into(),
            }
        ],
        status: "in-progress".into(),
        // ...
    },
    // ...
};

// Step 3: Finalize
let finalize_urp = URP {
    coordination: Coordination {
        workflow_id: Some("workflow-123".into()),
        step: Some("finalize".into()),
        step_index: Some(3),
        total_steps: Some(3),
        dependencies: vec![
            Dependency {
                step: "validate".into(),
                status: "completed".into(),
            },
            Dependency {
                step: "process".into(),
                status: "completed".into(),
            }
        ],
        status: "in-progress".into(),
        // ...
    },
    // ...
};
```

## Benefits of URP

### 1. Universal Message Format
- Single structure for all inter-component communication
- No custom message formats per integration
- Consistent handling across all components

### 2. Design-Runtime Alignment
- Runtime packets directly reference UDML specs
- Easy to verify that runtime behavior matches design
- Automated validation possible

### 3. Observability
- Rich metadata for tracing and debugging
- Complete context in every message
- Easy to build monitoring and analytics

### 4. Testability
- Mock URPs for testing
- Validate URPs against specs
- Replay URPs for debugging

### 5. Interoperability
- Language-agnostic JSON format
- Works across Rust, Python, JavaScript, etc.
- Standard tooling support

### 6. Evolvability
- Version fields allow evolution
- Backward compatibility through versioning
- Clear migration paths

## Best Practices

### 1. Always Validate URPs
Validate every URP against the JSON Schema before transmission and after reception.

### 2. Use ULID for IDs
Use ULID format for `urp_id`, `workflow_id`, and other identifiers for sortability and uniqueness.

### 3. Include Trace IDs
Always include `trace_id` for distributed tracing across components.

### 4. Set Appropriate Timeouts
Configure realistic `timeout_ms` values based on expected operation duration.

### 5. Handle Null Domains Gracefully
Some domains may be null (e.g., `extract` for simple CRUD operations). Handle these cases.

### 6. Secure Auth Tokens
Be careful with `auth_token` fields—log them carefully, encrypt in transit.

### 7. Document Custom Fields
If extending URPs with custom metadata, document extensions clearly.

### 8. Version Everything
Use version fields for URP spec, data schemas, and algorithm versions.

### 9. Correlate Related URPs
Use `correlation_id` to link related URPs (request-response, workflow steps).

### 10. Monitor URP Metrics
Track URP volume, latency, failures by route, component, and operation.

## Future Enhancements

### Potential Extensions

1. **Binary Format**: Protocol Buffers or MessagePack version for performance
2. **Compression**: Standard compression for large payloads
3. **Batching**: Multiple URPs in single transmission
4. **Streaming**: Support for streaming data in URPs
5. **Schema Evolution**: Automated schema migration tools
6. **Code Generation**: Generate URP serialization from UDML specs
7. **Validation Service**: Centralized URP validation service
8. **Replay Service**: URP recording and replay for testing

## Conclusion

The UDML Runtime Packet (URP) completes the UDML lifecycle by providing a standardized runtime representation that directly aligns with UDML design-time specifications. By using URPs, systems gain:

- **Universal interoperability** through a single message format
- **Strong validation** through JSON Schema and UDML reference checking
- **Complete observability** through rich metadata
- **Design-runtime traceability** through explicit UDML references

URP bridges the gap between architecture and execution, ensuring that systems operate exactly as designed and providing the foundation for automated validation, monitoring, and evolution.
