# UDML Schema Review and Standardization Question

Based on the UDML YAML files you created in response to the previous question (docs/UDML/components/question.md), please review all the YAML files created by different LLMs and work together to establish a standardized schema for UDML.

## Task

1. **Review all YAML files**: Examine the UDML YAML files created by all 7 LLMs across all component directories (claude-sonnet-4.5, GPT5, GPT5-codex, GPT5-mini, claude-sonnet-4.5-haiku, gemini-2.5-pro, grok-code-fast-1) and analyze the format patterns, structure consistency, and data representation approaches used.

2. **Identify common patterns**: Look for common structural elements, naming conventions, field types, and organizational patterns across all implementations.

3. **Propose standardized schema**: Based on your collective review, propose a standardized UDML schema that:
   - Defines consistent field names and types
   - Establishes common structural patterns
   - Provides clear guidelines for component descriptions
   - Ensures interoperability between different implementations

4. **Document schema decisions**: For each major decision in the schema, explain:
   - What variations were found across implementations
   - Why the chosen approach was selected
   - How it improves consistency and usability

5. **Provide schema specification**: Create a formal schema specification that includes:
   - Required vs optional fields
   - Data types and formats
   - Naming conventions
   - Structural guidelines
   - Example templates for different component types

## Context Files to Review

Please review UDML YAML files across all LLM directories:
- `docs/UDML/components/claude-sonnet-4.5/`
- `docs/UDML/components/GPT5/`
- `docs/UDML/components/GPT5-codex/`
- `docs/UDML/components/GPT5-mini/`
- `docs/UDML/components/claude-sonnet-4.5-haiku/`
- `docs/UDML/components/gemini-2.5-pro/`
- `docs/UDML/components/grok-code-fast-1/`

Key files to examine for each component:
- abk-agent.udml.yaml
- abk-checkpoint.udml.yaml
- abk-cli.udml.yaml
- abk-config.udml.yaml
- abk-executor.udml.yaml
- abk-lifecycle.udml.yaml
- abk-observability.udml.yaml
- abk-orchestration.udml.yaml
- abk-provider.udml.yaml
- cats.udml.yaml
- lifecycle-wasm.udml.yaml
- provider-wasm-tanbal.udml.yaml
- umf.udml.yaml

## Deliverable

Provide your collective analysis and schema proposal in a clear, structured response that includes:
- Summary of variations found across implementations
- Standardized schema specification covering the 6 UDML domains:
  - **Information**: Real world state and persistent semantic domain data
  - **Access**: Permissible ways data may be viewed/read (boundaries, auth, queries, visibility, indexing)
  - **Manipulation**: Lawful mutation rules (creation, change, deletion, invalidation, correction)
  - **Extract**: Derivation of new representations (inference, computation, aggregation, projection)
  - **Movement**: How data travels through processes, boundaries, runtimes, apps, microservices, protocols, APIs
  - **Coordination**: Synchronization, scheduling, dependency resolution, consensus, orchestration primitives
- Rationale for each major schema decision
- Example implementations following the new schema for each domain