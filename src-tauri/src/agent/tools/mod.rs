use std::collections::BTreeMap;

use jsonschema::validator_for;
use serde::Serialize;
use serde_json::Value;

use crate::agent::error::AgentError;

pub mod plan;
pub mod record;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum RiskLevel {
    #[serde(rename = "R0")]
    R0,
    #[serde(rename = "R1")]
    R1,
    #[serde(rename = "R2")]
    R2,
    #[serde(rename = "R3")]
    R3,
    #[serde(rename = "R4")]
    R4,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confirmation {
    Automatic,
    SummaryOrSetting,
    Required,
    NavigationOnly,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    RetrySafe,
    RequiredExactlyOnce,
    NoAutomaticRetry,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub version: &'static str,
    pub input_schema: Value,
    pub output_schema: Value,
    pub risk: RiskLevel,
    pub confirmation: Confirmation,
    pub supports_undo: bool,
    pub timeout_ms: u64,
    pub idempotency: Idempotency,
    pub data_permissions: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ToolOwnership {
    Typescript,
    Shadow,
    RustOwned,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListedTool {
    pub descriptor: ToolDescriptor,
    pub ownership: ToolOwnership,
}

#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<(String, String), ToolDescriptor>,
}

impl ToolRegistry {
    pub fn built_in() -> Self {
        let mut tools = BTreeMap::new();
        for descriptor in [plan::descriptor(), record::descriptor()] {
            tools.insert(
                (descriptor.name.to_string(), descriptor.version.to_string()),
                descriptor,
            );
        }
        Self { tools }
    }

    pub fn descriptors(&self) -> Vec<&ToolDescriptor> {
        self.tools.values().collect()
    }
    pub fn names(&self) -> Vec<&'static str> {
        self.tools.values().map(|d| d.name).collect()
    }
    pub fn get(&self, name: &str, version: &str) -> Result<&ToolDescriptor, AgentError> {
        if let Some(descriptor) = self.tools.get(&(name.to_string(), version.to_string())) {
            return Ok(descriptor);
        }
        if self.tools.keys().any(|(n, _)| n == name) {
            Err(AgentError::ToolVersionMismatch)
        } else {
            Err(AgentError::ToolNotFound)
        }
    }
    pub fn listed(&self, ownership: ToolOwnership) -> Vec<ListedTool> {
        self.descriptors()
            .into_iter()
            .cloned()
            .map(|descriptor| ListedTool {
                descriptor,
                ownership: ownership.clone(),
            })
            .collect()
    }
    pub fn validate_input(
        &self,
        descriptor: &ToolDescriptor,
        input: &Value,
    ) -> Result<(), AgentError> {
        validate(&descriptor.input_schema, input)
    }
    pub fn validate_output(
        &self,
        descriptor: &ToolDescriptor,
        output: &Value,
    ) -> Result<(), AgentError> {
        validate(&descriptor.output_schema, output)
    }
}

fn validate(schema: &Value, instance: &Value) -> Result<(), AgentError> {
    let validator = validator_for(schema).map_err(|_| AgentError::ToolSchemaInvalid)?;
    validator
        .validate(instance)
        .map_err(|_| AgentError::ToolSchemaInvalid)
}

#[cfg(test)]
mod tests {
    use super::record::RecordCheckinPlanInput;
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_has_unique_stable_names_and_complete_metadata() {
        let registry = ToolRegistry::built_in();
        let descriptors = registry.descriptors();
        let names: Vec<_> = descriptors.iter().map(|d| d.name).collect();
        assert_eq!(names, ["plan.get_today", "record.checkin_plan"]);
        for descriptor in descriptors {
            assert_eq!(descriptor.version, "1");
            assert!(descriptor.timeout_ms > 0);
            assert!(!descriptor.data_permissions.is_empty());
            assert_eq!(descriptor.input_schema["type"], "object");
            assert_eq!(descriptor.output_schema["type"], "object");
        }
    }

    #[test]
    fn checkin_schema_rejects_unlocked_and_invalid_fields() {
        let registry = ToolRegistry::built_in();
        let descriptor = registry.get("record.checkin_plan", "1").unwrap();
        let invalid = json!({"plan_id":"p", "duration_min":0, "finish":false});
        let error = registry.validate_input(descriptor, &invalid).unwrap_err();
        assert_eq!(error.code(), "tool_schema_invalid");
        let top_unknown =
            json!({"plan_id":"p", "duration_min":1, "subject_id":"s", "finish":false});
        assert!(registry.validate_input(descriptor, &top_unknown).is_err());
        let top_knowledge_point_unknown =
            json!({"plan_id":"p", "duration_min":1, "knowledge_point_id":"k", "finish":false});
        assert!(registry
            .validate_input(descriptor, &top_knowledge_point_unknown)
            .is_err());
        let nested_unknown = json!({"plan_id":"p", "duration_min":1, "finish":false, "wrong_questions":[{"subject_id":"s"}]});
        assert!(registry
            .validate_input(descriptor, &nested_unknown)
            .is_err());
        let nested_knowledge_point_unknown = json!({"plan_id":"p", "duration_min":1, "finish":false, "wrong_questions":[{"knowledge_point_id":"k"}]});
        assert!(registry
            .validate_input(descriptor, &nested_knowledge_point_unknown)
            .is_err());
        let nested: Result<RecordCheckinPlanInput, _> = serde_json::from_value(json!({
            "plan_id":"p", "duration_min":1, "finish":false,
            "wrong_questions":[{"knowledge_point_id":"k", "unexpected":1}]
        }));
        assert!(nested.is_err());
    }
}
