read docs/UDML/UDML.md and read docs/UDML/issue_mapping.md and read docs/coupled/tightly_coupled_hypothesis.md and read docs/coupled/data_flow_diagram.md could you generate UDML for 
- `abk::agent` (Agent runtime / wiring)
- `abk::cli` (CLI/bootstrap)
- `abk[config]` (Configuration loader)
- `abk[provider]` / `ProviderFactory` (LLM provider factory + trait)
- `abk[lifecycle]` (WASM lifecycle templates & classification)
- `abk[executor]` (Command executor)
- `abk[checkpoint]` (Checkpoint/session manager)
- `abk[orchestration]` (Workflow coordinator)
- `abk[observability]` (Logging/telemetry)
- `CATS` (Tool registry + tool implementations)
- `UMF` (InternalMessage, ContentBlock, ToolCall/ToolResult, ChatML helpers)
- `Provider-WASM / Tanbal` (WASM provider)
- `Lifecycle-WASM` (WASM lifecycle templates)

 what format do you suggest JSON /YAML/TOML/Datadog/edn ? after you decided on a format create files for all of them