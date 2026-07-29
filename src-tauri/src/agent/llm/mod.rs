// Rust LLM provider layer (M3 Part 1). Mirrors src/services/llm-adapter.ts so an
// existing user provider config works unchanged: POST {base}/chat/completions,
// body {model,messages,temperature,stream:false,tools}, Bearer auth, parse
// choices[0].message into content + tool_calls + usage.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod openai_compatible;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ProviderToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "tool_call_id")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderToolCall {
    pub id: String,
    pub function: ProviderFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderUsage {
    #[serde(default, rename = "prompt_tokens")]
    pub prompt_tokens: i64,
    #[serde(default, rename = "completion_tokens")]
    pub completion_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ProviderToolCall>,
    pub usage: ProviderUsage,
}

/// Project a tool descriptor into an OpenAI function-calling tool object.
pub fn tool_object(name: &str, description: &str, parameters: &Value) -> Value {
    serde_json::json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_object_projects_descriptor_into_function_schema() {
        let value = tool_object("plan.get_today", "today", &json!({"type": "object"}));
        assert_eq!(value["type"], "function");
        assert_eq!(value["function"]["name"], "plan.get_today");
        assert_eq!(value["function"]["parameters"]["type"], "object");
    }
}
