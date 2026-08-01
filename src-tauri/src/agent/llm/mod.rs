// Rust LLM provider layer (M3 Part 1). Mirrors src/services/llm-adapter.ts so an
// existing user provider config works unchanged: POST {base}/chat/completions,
// body {model,messages,temperature,stream:false,tools}, Bearer auth, parse
// choices[0].message into content + tool_calls + usage.

use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(test)]
use std::sync::Mutex;

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
    /// OpenAI requires `type:"function"` on each tool_call when the assistant
    /// message is echoed back; the field is parsed from the response and
    /// defaults to "function" so echo messages stay well-formed.
    #[serde(rename = "type", default = "default_function_type")]
    pub kind: String,
    pub function: ProviderFunction,
}

fn default_function_type() -> String {
    "function".to_owned()
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

/// The model-facing boundary, mirroring executor::ToolDispatcher: a real
/// OpenAI-compatible variant plus a `#[cfg(test)]` scripted variant so the
/// Planner loop can be exercised without a network. No trait, no extra crate.
pub(crate) enum LlmProvider {
    OpenAiCompatible(openai_compatible::OpenAiCompatibleProvider),
    #[cfg(test)]
    Synthetic(SyntheticProvider),
}

impl LlmProvider {
    /// Stream a model turn. Each content delta is forwarded to `on_chunk`;
    /// tool_calls are reassembled across deltas; the returned `ProviderResponse`
    /// carries the full content, tool calls, and usage once the stream ends.
    pub async fn chat_stream(
        &self,
        messages: &[ProviderMessage],
        tools: &[Value],
        on_chunk: &mut (dyn FnMut(&str) + Send),
    ) -> Result<ProviderResponse, crate::agent::error::AgentError> {
        match self {
            Self::OpenAiCompatible(provider) => {
                provider.chat_stream(messages, tools, on_chunk).await
            }
            #[cfg(test)]
            Self::Synthetic(provider) => {
                *provider
                    .last_request
                    .lock()
                    .expect("synthetic last_request lock") = Some(messages.to_vec());
                let response = provider.next_response();
                if let Some(content) = &response.content {
                    on_chunk(content);
                }
                Ok(response)
            }
        }
    }
}

#[cfg(test)]
pub(crate) struct SyntheticProvider {
    responses: Mutex<Vec<ProviderResponse>>,
    last_request: Mutex<Option<Vec<ProviderMessage>>>,
}

#[cfg(test)]
impl SyntheticProvider {
    pub(crate) fn scripted(responses: Vec<ProviderResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            last_request: Mutex::new(None),
        }
    }

    /// The most recent request this provider received, so tests can assert
    /// what the Planner actually sent (system prompt contents, tools, etc.).
    pub(crate) fn last_request(&self) -> Option<Vec<ProviderMessage>> {
        self.last_request
            .lock()
            .expect("synthetic last_request lock")
            .clone()
    }

    fn next_response(&self) -> ProviderResponse {
        let mut guard = self.responses.lock().expect("synthetic responses lock");
        if guard.is_empty() {
            ProviderResponse {
                content: Some("(scripted exhausted)".to_owned()),
                tool_calls: Vec::new(),
                usage: ProviderUsage::default(),
            }
        } else {
            guard.remove(0)
        }
    }
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

    #[test]
    fn assistant_tool_call_echo_serializes_function_type() {
        let message = ProviderMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![ProviderToolCall {
                id: "c1".into(),
                kind: "function".into(),
                function: ProviderFunction {
                    name: "plan.get_today".into(),
                    arguments: "{\"exam_id\":\"e1\"}".into(),
                },
            }]),
            tool_call_id: None,
        };
        let serialized = serde_json::to_value(&message).unwrap();
        assert_eq!(serialized["tool_calls"][0]["type"], "function");
    }
}
