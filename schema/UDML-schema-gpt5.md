# UDML Schema Standard v1.0 (2025-11-10)

This document proposes a standardized UDML (Universal Data Modeling Language) schema synthesized from variations typically seen across multiple LLM-generated YAMLs. It defines consistent structure, naming, data types, and conventions across the six UDML domains:

- Information: Real world state and persistent semantic domain data
- Access: Permissible ways data may be viewed/read (boundaries, auth, queries, visibility, indexing)
- Manipulation: Lawful mutation rules (creation, change, deletion, invalidation, correction)
- Extract: Derivation of new representations (inference, computation, aggregation, projection)
- Movement: How data travels through processes, boundaries, runtimes, apps, microservices, protocols, APIs
- Coordination: Synchronization, scheduling, dependency resolution, consensus, orchestration primitives

The goals are interoperability, clarity, and machine-checked consistency, while remaining human-friendly.

---

## Summary of variations observed (and normalized)

Common differences seen across implementations and how this standard resolves them:

- Key naming: snake_case vs camelCase vs kebab-case. → Standardize on snake_case for YAML keys; PascalCase for type/entity names.
- Identity fields: id vs uid vs name combinations. → Provide `metadata.id` (ULID), plus `name` (stable slug) and `title` (human label).
- Versioning: absent vs free-form. → Require SemVer `version` at the document root.
- Dates: free-form vs ISO strings. → Use RFC 3339/ISO 8601 (`YYYY-MM-DDThh:mm:ssZ`).
- References: `$ref` vs `ref` vs path lists. → Use `$ref` with URI/URI-fragment and typed selectors.
- Arrays vs maps for collections. → Prefer arrays for ordered collections (entities, operations, flows) and maps only for label-like key–value (`metadata.labels`).
- Enums: numeric vs string. → Use string enums for readability; specify allowed values.
- Types: ad hoc primitives vs vague. → Provide a common type system: scalar, complex (array/map), union, and `$ref`.
- Domain boundaries: mixed sections vs flat. → Keep the 6 domains as first-class top-level sections.

---

## Top-level document structure

Required fields unless noted optional.

- udml: string (SemVer of this schema spec), e.g., "1.0.0"
- kind: string ("component" | "system" | "domain" | "dataset"), depends on use
- name: string (slug, kebab-case recommended)
- title: string (human-friendly)
- version: string (SemVer of the modeled artifact)
- description: string (Markdown supported)
- metadata: object
  - id: string (ULID recommended)
  - created_at: string (RFC 3339)
  - updated_at: string (RFC 3339, optional)
  - authors: array of { name: string, email?: string, org?: string }
  - labels: map<string,string> (optional free-form)
  - license: string (SPDX id, optional)
  - source: array of { type: string, url: string, note?: string } (optional)
- links: array of { rel: string, href: string, title?: string, type?: string } (optional)
- schemas: array of embedded reusable type definitions (see Type System) (optional)
- information: InformationDomain (required)
- access: AccessDomain (optional)
- manipulation: ManipulationDomain (optional)
- extract: ExtractDomain (optional)
- movement: MovementDomain (optional)
- coordination: CoordinationDomain (optional)

---

## Naming conventions

- YAML keys: snake_case
- Type/entity names: PascalCase (e.g., `UserAccount`, `OrderItem`)
- Resource/action verbs: lower_snake (e.g., `read`, `write`, `delete`)
- Slugs/ids: kebab-case when used in URLs or `name` fields

---

## Type system

Scalar types:
- string, integer, number, boolean
- datetime (RFC 3339), date (YYYY-MM-DD), duration (ISO 8601), uuid, ulid

Composed types:
- array<T>
- map<K,V> (K is string)
- union<T1|T2|...>
- `$ref`: reference to a `schemas` item or external document (URI/fragment)

Constraints:
- required: boolean
- enum: array<string>
- pattern: regex (RE2 style recommended)
- min_length, max_length, minimum, maximum
- format: string (e.g., `email`, `uri`, `hostname`)
- default: any

---

## Domain specifications

### Information domain

Purpose: Define the persistent semantic data model, its entities, attributes, identifiers, and relationships.

Structure:
- entities: array<Entity>
- relationships: array<Relationship> (optional)
- policies: object (optional)
  - retention: array<RetentionPolicy>
  - quality_rules: array<QualityRule>
  - classification: array<Classification> (e.g., PII)
- ontology: array<Term> (optional, for tagging/semantic alignment)

Entity:
- name: string (PascalCase)
- summary: string
- description: string
- attributes: array<Attribute>
- identifiers: array<Identifier> (at least one)
- examples: array<object> (optional)

Attribute:
- name: string
- type: Type (see Type system)
- required: boolean (default: false)
- description: string (optional)
- constraints: object (optional; see Constraints)
- pii: boolean (optional)
- classification: enum ["public","internal","restricted","secret"] (optional)

Identifier:
- name: string
- attributes: array<string> (attribute names composing the key)
- kind: enum ["primary","unique","natural","surrogate"]

Relationship:
- name: string
- from: string (Entity name)
- to: string (Entity name)
- cardinality: enum ["1:1","1:N","N:1","M:N"]
- via: string (optional, join/bridge entity)
- description: string (optional)

RetentionPolicy:
- target: enum ["entity","attribute","record"]
- applies_to: string | array<string> (entity or attribute names)
- rule: string (e.g., `retain 7y`, `delete after 30d`) 
- basis: enum ["legal","business","operational"] (optional)

QualityRule:
- name: string
- target: string (entity/attribute)
- expression: string (e.g., SQL-like or JSONPath predicate)
- severity: enum ["warn","error"]

Classification:
- target: string (entity/attribute)
- level: enum ["public","internal","restricted","secret"]
- tags: array<string> (optional)

Example (Information):
```yaml
information:
  entities:
    - name: UserAccount
      summary: Registered end-user account.
      description: >
        Represents a user with authentication identity and profile.
      identifiers:
        - name: user_pk
          attributes: [id]
          kind: primary
      attributes:
        - name: id
          type: ulid
          required: true
          description: Stable globally unique id.
        - name: email
          type: string
          required: true
          constraints:
            format: email
        - name: created_at
          type: datetime
          required: true
  relationships:
    - name: UserOrders
      from: UserAccount
      to: Order
      cardinality: 1:N
```

---

### Access domain

Purpose: Define how information may be viewed/read, including auth, policies, queries, indices, and visibility.

Structure:
- subjects: array<Subject> (users, services, roles)
- permissions: array<Permission> (action on resource)
- access_rules: array<AccessRule>
- views: array<View> (read models / projections)
- indexing: array<Index> (optional)
- visibility: array<VisibilityRule> (optional)

Subject:
- name: string (role or principal id)
- kind: enum ["role","user","service"]
- attributes: map<string,string> (optional)

Permission:
- name: string
- action: enum ["read","list","search","download"]
- resource: string (entity or view name)

AccessRule:
- subject: string (Subject name)
- allow: array<string> (permission names)
- deny: array<string> (permission names, optional)
- condition: string (optional expression)

View:
- name: string
- target: string (entity)
- projection: array<string> (attribute names)
- filter: string (optional)
- order_by: array<string> (optional)
- materialized: boolean (default: false)

Index:
- name: string
- target: string (entity)
- keys: array<{ attribute: string, order?: enum ["asc","desc"] }>
- unique: boolean (default: false)

VisibilityRule:
- target: string (entity/attribute/view)
- level: enum ["public","internal","restricted","secret"]

Example (Access):
```yaml
access:
  subjects:
    - name: analyst
      kind: role
  permissions:
    - name: read_users
      action: read
      resource: UserAccount
  access_rules:
    - subject: analyst
      allow: [read_users]
  views:
    - name: user_public
      target: UserAccount
      projection: [id, created_at]
      filter: "exists(id)"
  indexing:
    - name: user_email_idx
      target: UserAccount
      keys: [{ attribute: email }]
      unique: true
```

---

### Manipulation domain

Purpose: Define lawful mutations—create/change/delete—with preconditions, invariants, and side effects.

Structure:
- operations: array<Operation>
- invariants: array<Invariant> (optional)
- workflows: array<Workflow> (optional)

Operation:
- name: string
- target: string (entity)
- kind: enum ["create","update","delete","upsert","invalidate","correct"]
- input_schema: `$ref` | inline type (attributes allowed)
- preconditions: array<string>
- effects: array<string> (semantic effects / events)
- idempotent: boolean (default: false)
- audit: boolean (default: true)

Invariant:
- name: string
- expression: string (must hold across operations)
- severity: enum ["warn","error"]

Workflow:
- name: string
- steps: array<{ op: string, on_success?: string, on_failure?: string }>

Example (Manipulation):
```yaml
manipulation:
  operations:
    - name: create_user
      target: UserAccount
      kind: create
      input_schema:
        type: map
        properties:
          email: { type: string, constraints: { format: email } }
      preconditions: ["not exists(UserAccount where email = input.email)"]
      effects: ["UserCreated"]
      idempotent: false
  invariants:
    - name: user_email_unique
      expression: "unique(UserAccount.email)"
      severity: error
```

---

### Extract domain

Purpose: Define derivations, aggregations, projections, and computations.

Structure:
- derivations: array<Derivation>
- aggregations: array<Aggregation>
- materializations: array<Materialization> (optional)

Derivation:
- name: string
- source: string | array<string> (entities/views)
- transform: string (DSL or reference; e.g., SQL)
- output_schema: `$ref` | inline type
- freshness: { max_lag: duration } (optional)

Aggregation:
- name: string
- source: string | array<string>
- group_by: array<string>
- measures: array<{ name: string, expr: string }>
- filter: string (optional)

Materialization:
- name: string
- from: string (derivation/aggregation name)
- store: { type: string, location: string }
- schedule: string (cron or ISO 8601 repeating interval)

Example (Extract):
```yaml
extract:
  derivations:
    - name: active_users
      source: UserAccount
      transform: |
        SELECT id, created_at FROM UserAccount WHERE active = true
      output_schema:
        type: array
        items:
          $ref: "#/information/entities/UserAccount"
```

---

### Movement domain

Purpose: Describe data in motion: flows, protocols, formats, QoS, and delivery semantics.

Structure:
- flows: array<Flow>
- connectors: array<Connector> (optional)

Flow:
- name: string
- source: Endpoint
- sink: Endpoint
- format: enum ["json","csv","avro","parquet","protobuf","binary"]
- protocol: enum ["http","https","grpc","kafka","s3","gcs","nats","amqp","filesystem"]
- frequency: enum ["event","batch","stream"]
- delivery: enum ["at-most-once","at-least-once","exactly-once"]
- retry: { max_retries: integer, backoff: duration } (optional)
- dlq: { type: string, location: string } (optional)
- schema: `$ref` | inline type (optional)

Endpoint:
- type: enum ["topic","queue","url","bucket","path","table"]
- name: string
- uri: string (optional)
- auth_ref: string (optional reference to access subject/secret)

Connector:
- name: string
- kind: enum ["source","sink","transform"]
- config: map<string, any>

Example (Movement):
```yaml
movement:
  flows:
    - name: user_events_to_dw
      source: { type: topic, name: user.events }
      sink: { type: table, name: dw.user_events }
      format: json
      protocol: kafka
      frequency: stream
      delivery: at-least-once
      retry: { max_retries: 10, backoff: PT10S }
```

---

### Coordination domain

Purpose: Define scheduling, orchestration, dependencies, consensus, and idempotency controls.

Structure:
- schedules: array<Schedule>
- triggers: array<Trigger>
- dependencies: array<Dependency>
- state_machines: array<StateMachine> (optional)
- consensus: array<ConsensusConfig> (optional)

Schedule:
- name: string
- cron: string
- target: string (operation/materialization/flow)

Trigger:
- name: string
- on_event: string (event name)
- target: string (operation/workflow)
- condition: string (optional)

Dependency:
- name: string
- requires: array<string> (names of other tasks)
- policy: enum ["all","any"]

StateMachine:
- name: string
- states: array<string>
- transitions: array<{ from: string, to: string, on: string, guard?: string }>

ConsensusConfig:
- name: string
- algorithm: enum ["raft","paxos","zab","multi-paxos","epaxos"]
- quorum: integer
- notes: string (optional)

Example (Coordination):
```yaml
coordination:
  schedules:
    - name: nightly_user_snapshot
      cron: "0 2 * * *"
      target: materialization:active_users
  triggers:
    - name: on_user_created
      on_event: UserCreated
      target: operation:create_user
  dependencies:
    - name: publish_dw
      requires: [nightly_user_snapshot]
      policy: all
```

---

## `$ref` and cross-document linking

- Use `$ref` for reusable schemas. Prefer local fragments (e.g., `#/schemas/UserAccount`), but allow absolute/relative URIs.
- For entity references, the canonical anchor may be `#/information/entities/<EntityName>`.
- For external packages, include a `links` entry with `rel: schema` and reuse its URL via `$ref`.

Example:
```yaml
schemas:
  - name: UserId
    type: ulid

manipulation:
  operations:
    - name: delete_user
      target: UserAccount
      kind: delete
      input_schema:
        $ref: "#/schemas/UserId"
```

---

## Required vs optional summary

Required at root:
- udml, kind, name, title, version, description, metadata.id, metadata.created_at, information

Optional but recommended:
- metadata.authors, metadata.labels, links, schemas
- access, manipulation, extract, movement, coordination

Within domains:
- Information.entities[].name, .attributes, .identifiers (≥1)
- Access.subjects/permissions/rules as applicable to governed systems
- Manipulation.operations for systems with mutations
- Extract.derivations/aggregations for analytical systems
- Movement.flows for data-in-motion
- Coordination schedules/triggers when orchestration exists

---

## Schema decisions and rationale

- Snake_case YAML keys: Easiest to read/edit; avoids YAML dash/camel confusion; aligns with many config ecosystems.
- PascalCase type/entity names: Conventional for conceptual models; improves readability in docs and diagrams.
- ULID for IDs: Lexicographically sortable, unique, URL-safe; stable across stores; UUID also allowed via type.
- RFC 3339 datetimes: Interoperable across languages and databases.
- Arrays for ordered collections: Preserve author intent and diff stability; maps reserved for label-like KV.
- String enums: Human-friendly diffs and comments; validates well; avoids magic numbers.
- `$ref` everywhere: Enables reuse and modularity; compatible with JSON Schema tools and linters.
- Six-domain separation: Keeps responsibilities clear; supports mixed operational/analytical use-cases without conflation.

---

## Minimal templates per domain

Root template:
```yaml
udml: "1.0.0"
kind: component
name: example-component
title: Example Component
version: 0.1.0
description: |
  Short description in Markdown.
metadata:
  id: 01JABCD3EFG4567H89JK1MN2OP
  created_at: 2025-11-10T00:00:00Z
  authors:
    - name: Jane Doe
      email: jane@example.com
links: []
schemas: []
information: { entities: [], relationships: [] }
```

Information template:
```yaml
information:
  entities:
    - name: EntityName
      summary: One-liner.
      description: Longer text.
      identifiers: [{ name: pk, attributes: [id], kind: primary }]
      attributes:
        - { name: id, type: ulid, required: true }
```

Access template:
```yaml
access:
  subjects: []
  permissions: []
  access_rules: []
  views: []
```

Manipulation template:
```yaml
manipulation:
  operations: []
  invariants: []
```

Extract template:
```yaml
extract:
  derivations: []
  aggregations: []
  materializations: []
```

Movement template:
```yaml
movement:
  flows: []
```

Coordination template:
```yaml
coordination:
  schedules: []
  triggers: []
  dependencies: []
```

---

## Validation guidance

- Author a JSON Schema for this UDML spec to enable automated linting (future work).
- Until then, adopt CI checks: YAML parse, key whitelist, enums, datetime formats, and `$ref` resolution.
- Recommend treating `information.entities[].identifiers` and `attributes[].required` as hard-fail validations.

---

## Change management

- Use SemVer for `udml` (schema) and `version` (artifact) separately.
- Backwards-compatible extensions add optional fields or new enum members.
- Breaking changes increment `udml` major version and provide a migration document.

---

## Appendix: Example end-to-end snippet

```yaml
udml: "1.0.0"
kind: component
name: user-service
title: User Service
version: 1.2.3
description: Manages user accounts, identities, and profiles.
metadata:
  id: 01JD1M2P3Q4R5S6T7U8V9WXYZ0
  created_at: 2025-11-10T12:00:00Z
  authors: [{ name: "Team Identity" }]

information:
  entities:
    - name: UserAccount
      summary: Registered account.
      description: A user with credentials and profile.
      identifiers: [{ name: user_pk, attributes: [id], kind: primary }]
      attributes:
        - { name: id, type: ulid, required: true }
        - { name: email, type: string, required: true, constraints: { format: email } }
        - { name: created_at, type: datetime, required: true }

access:
  subjects: [{ name: analyst, kind: role }]
  permissions: [{ name: read_users, action: read, resource: UserAccount }]
  access_rules: [{ subject: analyst, allow: [read_users] }]

manipulation:
  operations:
    - name: create_user
      target: UserAccount
      kind: create
      input_schema:
        type: map
        properties:
          email: { type: string, constraints: { format: email } }
      preconditions: ["not exists(UserAccount where email = input.email)"]
      effects: ["UserCreated"]

extract:
  derivations:
    - name: recent_users
      source: UserAccount
      transform: |
        SELECT id, email FROM UserAccount WHERE created_at > now() - interval '7 day'
      output_schema: { type: array, items: { type: map, properties: { id: { type: ulid }, email: { type: string } } } }

movement:
  flows:
    - name: user_events
      source: { type: topic, name: user.events }
      sink: { type: bucket, name: s3://analytics/user_events/ }
      format: json
      protocol: kafka
      frequency: stream
      delivery: at-least-once

coordination:
  schedules:
    - name: nightly_extract
      cron: "0 2 * * *"
      target: materialization:recent_users
```
