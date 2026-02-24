use std::sync::Arc;
use oclaws_agent_core::agent::{Agent, AgentConfig, ToolExecutor};
use oclaws_llm_core::chat::{Tool, ToolFunction};
use oclaws_llm_core::providers::LlmProvider;
use oclaws_tools_core::tool::ToolRegistry;
use oclaws_plugin_core::PluginRegistrations;

/// Bridges tools-core's ToolRegistry to agent-core's ToolExecutor trait.
/// Also merges plugin-registered tools when available.
pub struct ToolRegistryExecutor {
    registry: Arc<ToolRegistry>,
    plugin_regs: Option<Arc<PluginRegistrations>>,
}

impl ToolRegistryExecutor {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry, plugin_regs: None }
    }

    pub fn with_plugin_registrations(mut self, regs: Arc<PluginRegistrations>) -> Self {
        self.plugin_regs = Some(regs);
        self
    }
}

#[async_trait::async_trait]
impl ToolExecutor for ToolRegistryExecutor {
    async fn execute(&self, name: &str, arguments: &str) -> Result<String, String> {
        // Try built-in registry first
        if self.registry.has_tool(name) {
            let args: serde_json::Value = serde_json::from_str(arguments)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            let call = oclaws_tools_core::tool::ToolCall {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.to_string(),
                arguments: args,
            };
            let resp = self.registry.execute_call(call).await;
            return if let Some(err) = resp.error {
                Err(err)
            } else {
                Ok(serde_json::to_string(&resp.result).unwrap_or_default())
            };
        }

        // Fall back to plugin tools
        if let Some(regs) = &self.plugin_regs {
            let tools = regs.tools.read().await;
            if let Some(tool) = tools.iter().find(|t| t.name == name) {
                let params: serde_json::Value = serde_json::from_str(arguments)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                return match tool.executor.execute(params).await {
                    Ok(val) => Ok(serde_json::to_string(&val).unwrap_or_default()),
                    Err(e) => Err(e),
                };
            }
        }

        Err(format!("Tool '{}' not found", name))
    }

    fn available_tools(&self) -> Vec<Tool> {
        let mut tools: Vec<Tool> = self.registry.list_for_llm().into_iter().filter_map(|v| {
            Some(Tool {
                type_: "function".into(),
                function: ToolFunction {
                    name: v["name"].as_str()?.to_string(),
                    description: v["description"].as_str()?.to_string(),
                    parameters: v["parameters"].clone(),
                },
            })
        }).collect();

        // Merge plugin tools (blocking read via try_read to avoid async in sync fn)
        if let Some(regs) = &self.plugin_regs {
            if let Ok(plugin_tools) = regs.tools.try_read() {
                for pt in plugin_tools.iter() {
                    tools.push(Tool {
                        type_: "function".into(),
                        function: ToolFunction {
                            name: pt.name.clone(),
                            description: pt.description.clone(),
                            parameters: pt.input_schema.clone(),
                        },
                    });
                }
            }
        }

        tools
    }
}

/// Run a single user message through the Agent with tools, returning the final reply.
pub async fn agent_reply(
    provider: &Arc<dyn LlmProvider>,
    tool_executor: &ToolRegistryExecutor,
    user_input: &str,
) -> Result<String, String> {
    let model = provider.default_model().to_string();
    let tool_names: Vec<String> = tool_executor.available_tools()
        .iter().map(|t| t.function.name.clone()).collect();
    let prompt = format!(
        "You are a helpful assistant with tools: {}. You CAN access the internet. \
         Use web_fetch for APIs/simple pages, browse for JS-heavy sites. Respond in the user's language.",
        tool_names.join(", ")
    );
    let config = AgentConfig::new("channel-agent", &model, "default")
        .with_system_prompt(&prompt);
    let mut agent = Agent::new(config, provider.clone());
    agent.initialize().await.map_err(|e| e.to_string())?;
    agent.run_with_tools(user_input, tool_executor).await.map_err(|e| e.to_string())
}
