use std::{collections::HashMap, fmt::Debug, sync::Arc};

use anyhow::Result;
use rmcp::{
    model::{CallToolRequestParam, CallToolResult, Tool as McpTool},
    service::ServerSink,
};
use serde_json::Value;

#[allow(async_fn_in_trait)]
pub trait Tool: Send + Sync {
    fn name(&self) -> String;
    fn description(&self) -> String;
    fn parameters(&self) -> Value;
    async fn call(&self, args: Value) -> Result<CallToolResult>;
}

pub struct McpToolAdapter {
    tool: McpTool,
    call_mcp_message: String,
    server: ServerSink,
}

impl McpToolAdapter {
    pub fn new(tool: McpTool, call_mcp_message: String, server: ServerSink) -> Self {
        Self {
            tool,
            call_mcp_message,
            server,
        }
    }
}

impl McpToolAdapter {
    pub fn call_mcp_message(&self) -> &str {
        &self.call_mcp_message
    }
}

impl Tool for McpToolAdapter {
    fn name(&self) -> String {
        self.tool.name.clone().to_string()
    }

    fn description(&self) -> String {
        self.tool
            .description
            .to_owned()
            .unwrap_or_default()
            .to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::to_value(&self.tool.input_schema).unwrap_or(serde_json::json!({}))
    }

    async fn call(&self, args: Value) -> Result<CallToolResult> {
        let arguments = match args {
            Value::Object(map) => Some(map),
            _ => None,
        };
        log::debug!("arguments: {:?}", arguments);
        let call_result = self
            .server
            .call_tool(CallToolRequestParam {
                name: self.tool.name.clone(),
                arguments,
            })
            .await?;

        Ok(call_result)
    }
}

pub struct ToolSet<T: Tool> {
    tools: HashMap<String, Arc<T>>,
}

impl<T: Tool> Clone for ToolSet<T> {
    fn clone(&self) -> Self {
        ToolSet {
            tools: self.tools.clone(),
        }
    }
}

impl<T: Tool> Default for ToolSet<T> {
    fn default() -> Self {
        ToolSet {
            tools: HashMap::new(),
        }
    }
}

impl<T: Tool> Debug for ToolSet<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolSet")
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl<T: Tool> ToolSet<T> {
    pub fn add_tool(&mut self, tool: T) {
        self.tools.insert(tool.name(), Arc::new(tool));
    }

    pub fn get_tool(&self, name: &str) -> Option<Arc<T>> {
        self.tools.get(name).cloned()
    }

    pub fn tools(&self) -> Vec<Arc<T>> {
        self.tools.values().cloned().collect()
    }
}
