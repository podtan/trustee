# Unified Data Morphism Language (UDML)

## Abstract

All software systems, regardless of paradigm, can be fully described and constructed using only **data** and the **rules that govern how data is accessed, manipulated, extracted, moved, and coordinated**. Behavior and architecture are secondary representations of these primary data relationships.

## Core Principle

> **The only truly fundamental primitive in software is data, and all software is the controlled morphing of data.**

## The Six Data Domains

| Domain         | Definition |
|----------------|------------|
| **Information** | The real world state represented in software. Persistent semantic domain data. |
| **Access**      | The permissible ways this data may be viewed or read. Includes boundaries, auth, queries, visibility and indexing. |
| **Manipulation**| The lawful mutation rules of data: creation, change, deletion, invalidation or correction. |
| **Extract**     | Derivation of new representations of data from existing data or environment. Includes inference, computation, aggregation, projection. |
| **Movement**    | How data travels through processes, boundaries, runtimes, apps, microservices, protocols, and APIs. Routing + distribution. |
| **Coordination**| The primitives and rules for synchronization, scheduling, dependency resolution, consensus, and orchestration between actors, components, or nodes. |

## Hypothesis

Any software system can be synthesized, analyzed, and reasoned about entirely by:

1. Defining all **Information** forms
2. Defining all **Access** rules
3. Defining all **Manipulation** rules
4. Defining all **Extract** rules
5. Defining all **Movement** rules
6. Defining all **Coordination** primitives

…and an engine capable of interpreting these categories can generate the complete executable system — regardless of implementation language, paradigm, or runtime.

## UDML specification

- A compact, machine-readable language that lists: information schemas, access policies, manipulation rules, extract transforms, movement routes, and coordination primitives.
- Recommended formats: YAML/JSON (schema + policy sections) so engines and validators can interpret and compile behaviors into concrete runtimes.
- Conformance: validators, interpreters, and target adapters translate UDML specs into imperative, declarative, actor-based, or orchestration code.

## Implications

- Architecture reduces to data calculus.
- State safety becomes rule safety.
- Code generation becomes more deterministic as design space is expressed as composable data rules.
- LLMs and spec-driven engines become reliable system builders because the design is structured and machine-readable.

## Modular

Two questions. Two rules.

1. Does this need different ownership? → New module
2. Does this need new movement or coordination? → New movement/coordination rule (and evaluate module split)

Access, Manipulation, and Extract remain within the owning module unless coordination or movement requires cross-module concerns.

## SQL Comparison

**Information** = DDL
**Access** = DCL
**Manipulation** DML
**Extract** DQL
**Movement** ?
**Coordination** ?

## 🌐 Exposure and Integration Guidelines

UDML modules are designed for **uniform internal communication** using the **UDML Runtime Packet (URP)**.  
Each module exposes a single interface (e.g., `handle(URP)`) that accepts and returns URPs, ensuring consistent data-driven interaction.

To make module capabilities available to **external systems** (such as REST, gRPC, GraphQL, WebSocket, or CLI tools), developers should use a **UDML Gateway**.

### UDML Gateway Pattern

A UDML Gateway acts as an adapter layer between protocol-specific clients and the internal UDML runtime:

1. **Receives** external protocol requests (e.g., REST or gRPC calls).  
2. **Transforms** them into a valid URP instance.  
3. **Routes** the URP to the appropriate module’s `handle(URP)` method.  
4. **Returns** the resulting URP (or extracted output) to the caller in their expected protocol format.

This pattern maintains:
- Architectural purity and uniformity across all modules  
- Clear separation between **internal morphic logic** and **external integration surfaces**  
- Observability and traceability in distributed environments  
- Compatibility with OpenAPI, gRPC, and other standard interface tools

### Important Design Rule

> **Do not** expose multiple API endpoints or protocol-specific operations directly from UDML modules.  
>  
> **Instead**, expose all public-facing interfaces through a UDML Gateway that maps external requests to URPs.

This approach prevents fragmentation, preserves UDML’s data calculus model, and ensures complete alignment between **design-time UDML specifications** and **runtime module behavior**.

