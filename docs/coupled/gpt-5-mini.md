# GPT-5 Mini: Decoupling Strategy for Trustee Agent Architecture

## Core Problem Analysis

The Trustee project exhibits **architectural schizophrenia** - it claims modularity through separate crates (ABK, CATS, UMF) but implements tight coupling through feature flags and transitive dependencies. This creates a monolithic build where "modular" components are actually compile-time features of a single large crate.

## Current Coupling Points

1. **Feature Gate Dependencies**: CATS and UMF are optional dependencies of ABK, activated via the `agent` feature flag
2. **Transitive Dependency Hiding**: Trustee depends only on `abk = { features = ["agent"] }`, masking the actual dependency tree
3. **Direct Imports**: ABK's agent module directly imports `cats::create_tool_registry_with_open_window_size` and `umf::chatml::ChatMLFormatter`
4. **Version Lockstep**: All components must be version-compatible within a single ABK release

## GPT-5 Mini Solution: Plugin-Based Agent Architecture

### Architecture Vision

Transform Trustee into a **true plugin-based system** where agent capabilities are loaded dynamically rather than compiled statically. This creates genuine modularity with independent deployment, versioning, and runtime composition.

### Key Innovations

#### 1. **Capability Interfaces as WASM Plugins**

Instead of feature-gated compilation, define agent capabilities as WASM interfaces:

```rust
// Capability trait definitions
pub trait ToolProvider {
    fn get_tools(&self) -> Vec<ToolDefinition>;
    fn execute_tool(&self, name: &str, params: Value) -> Result<Value>;
}

pub trait MessageFormatter {
    fn format_messages(&self, messages: &[Message]) -> String;
    fn parse_response(&self, response: &str) -> Result<ParsedResponse>;
}

pub trait LifecycleManager {
    fn classify_task(&self, task: &str) -> AgentType;
    fn get_workflow(&self, agent_type: AgentType) -> WorkflowDefinition;
}
```

#### 2. **Runtime Capability Discovery**

Replace compile-time features with runtime plugin loading:

```rust
pub struct AgentRuntime {
    tool_providers: HashMap<String, Box<dyn ToolProvider>>,
    formatters: HashMap<String, Box<dyn MessageFormatter>>,
    lifecycles: HashMap<AgentType, Box<dyn LifecycleManager>>,
}

impl AgentRuntime {
    pub async fn load_capability(&mut self, plugin_path: &Path) -> Result<()> {
        let plugin = WasmPlugin::load(plugin_path).await?;
        match plugin.capability_type() {
            CapabilityType::Tools => {
                self.tool_providers.insert(plugin.name(), plugin.as_tool_provider()?);
            }
            CapabilityType::Formatting => {
                self.formatters.insert(plugin.name(), plugin.as_formatter()?);
            }
            CapabilityType::Lifecycle => {
                let lifecycle = plugin.as_lifecycle()?;
                self.lifecycles.insert(lifecycle.agent_type(), lifecycle);
            }
        }
        Ok(())
    }
}
```

#### 3. **Independent Crate Deployment**

Each capability becomes a separately deployable crate with its own release cycle:

```
trustee-core/           # Core orchestration (no capabilities)
├── Cargo.toml         # Only depends on wasmtime, serde, tokio
└── src/
    └── runtime.rs     # Plugin loading and orchestration

trustee-tools/         # Tool implementations
├── Cargo.toml         # Independent versioning
└── src/
    └── tools.rs       # File ops, execution, etc.

trustee-formatting/    # Message formatting
├── Cargo.toml         # Independent versioning
└── src/
    └── formatters.rs  # ChatML, streaming, etc.

trustee-lifecycles/    # Agent morphing logic
├── Cargo.toml         # Independent versioning
└── src/
    └── lifecycles.rs  # Classification, workflows
```

#### 4. **Configuration-Driven Composition**

Replace feature flags with declarative configuration:

```toml
# trustee.toml
[capabilities]
tools = "trustee-tools = \"1.2.3\""
formatting = "trustee-formatting = \"2.1.0\""
lifecycle = "trustee-lifecycles = \"1.0.5\""

[agent_types.coding]
tools = ["file_ops", "execution", "search"]
formatter = "chatml"
lifecycle = "coding_workflow"

[agent_types.research]
tools = ["web_search", "analysis"]
formatter = "streaming"
lifecycle = "research_workflow"
```

#### 5. **Plugin Marketplace**

Create a plugin ecosystem where capabilities can be:
- **Developed independently** by different teams
- **Versioned separately** with semantic versioning
- **Discovered dynamically** at runtime
- **Hot-swappable** without recompiling the core agent

### Benefits of This Approach

#### **True Modularity**
- Each capability can be updated independently
- No monolithic rebuilds for feature changes
- Clear separation of concerns with interface contracts

#### **Runtime Flexibility**
- Load only needed capabilities for specific tasks
- Switch implementations without recompilation
- A/B test different capability versions

#### **Ecosystem Enablement**
- Third-party plugins for specialized domains
- Community contributions without core modifications
- Domain-specific optimizations

#### **Development Velocity**
- Parallel development of capabilities
- Independent testing and deployment
- Faster iteration cycles

### Migration Strategy

#### Phase 1: Interface Extraction
1. Define WASM capability interfaces
2. Extract current ABK features into separate crates
3. Create plugin loader infrastructure

#### Phase 2: Runtime Loading
1. Implement plugin discovery and loading
2. Replace feature flags with configuration
3. Add capability negotiation protocols

#### Phase 3: Ecosystem Building
1. Publish capability crates independently
2. Create plugin registry/discovery mechanism
3. Develop tooling for plugin development

### Risk Mitigation

#### **Interface Stability**
- Semantic versioning for capability interfaces
- Backward compatibility guarantees
- Interface evolution strategies

#### **Performance Overhead**
- Plugin loading cached and optimized
- Zero-cost abstraction for hot paths
- Lazy loading of unused capabilities

#### **Debugging Complexity**
- Plugin isolation prevents cascading failures
- Comprehensive logging and tracing
- Development tools for plugin debugging

## Conclusion

The GPT-5 Mini approach transforms Trustee from a tightly-coupled monolith into a genuinely modular, plugin-based agent platform. By moving from compile-time features to runtime capabilities, we achieve true separation of concerns while enabling an ecosystem of independently developed and deployed agent components.

This architecture positions Trustee as a **composable agent platform** rather than a single-purpose tool, enabling it to adapt to new domains and capabilities without architectural rewrites.</content>
<parameter name="filePath">/data/Projects/podtan/trustee/docs/coupled/gpt-5-mini.md