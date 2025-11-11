## 📜 Proposal: Add UDML Runtime Packet (URP) to the UDML Specification

### Summary
As part of the UDML evolution, we propose adding a **runtime execution model** called **UDML Runtime Packet (URP)**.

URP represents a **lightweight, runtime instance of a UDML component**, enabling all modules to communicate using a single, standardized data structure that contains the six UDML domains.

### Rationale
Currently, UDML defines the *design-time specification* of software systems (schemas, rules, flows).  
URP introduces the *runtime representation* of those definitions — a unified message envelope that modules exchange during execution.

This addition will complete the UDML lifecycle by bridging **design-time → runtime**, ensuring that runtime behaviors directly align with the static UDML specifications.

### Specification Path
Add the URP definition to:  
`docs/UDML/URP/URP.md`  
and reference it from the core guide under **"Runtime Execution Model"**.

### Core Concept
A **UDML Runtime Packet (URP)** is a universal, language-agnostic structure that includes the six UDML domains and must **align structurally and semantically with the UDML specification**.  
Each URP instance represents a live, runtime realization of UDML-defined information and rules.

The following JSON examples are **illustrative only**.  
The UDML specification owner should define the **final canonical schema** and field conventions to ensure perfect alignment with the UDML core spec.

```json
{
  "information": { "id": "entity-id", "data": { /* payload or state snapshot */ } },
  "access": { "auth": "bearer", "roles": ["service-a", "service-b"], "visibility": "internal" },
  "manipulation": { "operation": "append-message", "preconditions": ["session-exists"] },
  "extract": { "transform": "generate-summary", "cacheable": true },
  "movement": { "from": "module-a", "to": "module-b", "protocol": "grpc", "async": true },
  "coordination": { "workflow": "session-lifecycle", "retry": { "max": 3, "backoff": "exponential" } }
}


## 🧾 JSON Schema Requirement
The URP specification at docs/UDML/URP/URP.md **must include an official JSON Schema** describing the URP structure, validation rules, and domain constraints.  
This ensures:
- Schema-driven interoperability across languages and runtimes  
- Automated validation of URPs before transmission or processing  
- Compatibility with existing tooling (OpenAPI, JSON Schema validators, LLM parsers)  

**Suggested file path:**  
`/docs/UDML/URP/URP.schema.json`

The schema should explicitly define:
- Required six domain properties (`information`, `access`, `manipulation`, `extract`, `movement`, `coordination`)  
- Allowed data types and references to corresponding UDML domain specs  
- Extensibility rules for optional metadata fields  
- Versioning alignment with the main UDML specification  
- Validation examples for cross-domain consistency (e.g., `movement.to` references a valid module in `information`)  

The JSON Schema becomes the **normative runtime contract** ensuring that all URP instances are consistent, verifiable, and faithfully aligned with the UDML core specification.
