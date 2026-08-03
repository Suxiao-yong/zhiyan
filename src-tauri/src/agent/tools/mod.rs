use std::collections::BTreeMap;

use jsonschema::validator_for;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::error::AgentError;

pub mod exam;
pub mod plan;
pub mod record;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confirmation {
    Automatic,
    SummaryOrSetting,
    Required,
    NavigationOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
    tools: BTreeMap<&'static str, ToolDescriptor>,
}

impl ToolRegistry {
    pub fn built_in() -> Self {
        let mut tools = BTreeMap::new();
        for descriptor in [
            plan::descriptor(),
            plan::get_range_descriptor(),
            record::descriptor(),
            record::get_history_descriptor(),
            exam::descriptor(),
        ] {
            tools.insert(descriptor.name, descriptor);
        }
        Self { tools }
    }

    #[cfg(test)]
    pub(crate) fn for_test(descriptors: Vec<ToolDescriptor>) -> Self {
        Self {
            tools: descriptors
                .into_iter()
                .map(|descriptor| (descriptor.name, descriptor))
                .collect(),
        }
    }

    pub fn descriptors(&self) -> Vec<&ToolDescriptor> {
        self.tools.values().collect()
    }
    pub fn names(&self) -> Vec<&'static str> {
        self.tools.values().map(|d| d.name).collect()
    }
    pub fn get(&self, name: &str, version: &str) -> Result<&ToolDescriptor, AgentError> {
        let descriptor = self.tools.get(name).ok_or(AgentError::ToolNotFound)?;
        if descriptor.version == version {
            Ok(descriptor)
        } else {
            Err(AgentError::ToolVersionMismatch)
        }
    }
    pub fn validate_input(
        &self,
        name: &str,
        version: &str,
        input: &Value,
    ) -> Result<(), AgentError> {
        let descriptor = self.get(name, version)?;
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
        assert_eq!(
            names,
            [
                "exam.get_active",
                "plan.get_range",
                "plan.get_today",
                "record.checkin_plan",
                "record.get_history"
            ]
        );
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
        let invalid = json!({"plan_id":"p", "duration_min":0, "finish":false});
        let error = registry
            .validate_input("record.checkin_plan", "1", &invalid)
            .unwrap_err();
        assert_eq!(error.code(), "tool_schema_invalid");
        let top_unknown =
            json!({"plan_id":"p", "duration_min":1, "subject_id":"s", "finish":false});
        assert!(registry
            .validate_input("record.checkin_plan", "1", &top_unknown)
            .is_err());
        let top_knowledge_point_unknown =
            json!({"plan_id":"p", "duration_min":1, "knowledge_point_id":"k", "finish":false});
        assert!(registry
            .validate_input("record.checkin_plan", "1", &top_knowledge_point_unknown)
            .is_err());
        let nested_unknown = json!({"plan_id":"p", "duration_min":1, "finish":false, "wrong_questions":[{"subject_id":"s"}]});
        assert!(registry
            .validate_input("record.checkin_plan", "1", &nested_unknown)
            .is_err());
        let nested_knowledge_point_unknown = json!({"plan_id":"p", "duration_min":1, "finish":false, "wrong_questions":[{"knowledge_point_id":"k"}]});
        assert!(registry
            .validate_input("record.checkin_plan", "1", &nested_knowledge_point_unknown)
            .is_err());
        let nested: Result<RecordCheckinPlanInput, _> = serde_json::from_value(json!({
            "plan_id":"p", "duration_min":1, "finish":false,
            "wrong_questions":[{"knowledge_point_id":"k", "unexpected":1}]
        }));
        assert!(nested.is_err());
    }

    #[test]
    fn plan_output_fixture_is_valid_and_missing_field_is_rejected() {
        let registry = ToolRegistry::built_in();
        let plan = json!({
            "id":"p1", "exam_id":"e1", "subject_id":"s1", "knowledge_point_id":null,
            "date":"2026-07-18", "planned_tasks":null, "planned_duration":30,
            "actual_duration":null, "actual_tasks":null, "status":"pending", "generated_by":"local",
            "ai_suggestion":null, "user_modified":0, "sort_order":0, "created_at":"now", "updated_at":"now",
            "subject_name":"Math", "knowledge_point_name":null, "record_count":0
        });
        let expected_output = json!({"business_date":"2026-07-18", "plans":[plan]});
        let descriptor = registry.get("plan.get_today", "1").unwrap();
        assert!(registry
            .validate_output(descriptor, &expected_output)
            .is_ok());
        let mut missing = expected_output.clone();
        missing["plans"][0]
            .as_object_mut()
            .unwrap()
            .remove("record_count");
        assert!(registry.validate_output(descriptor, &missing).is_err());
    }

    #[test]
    fn valid_record_input_passes_schema_validation() {
        let registry = ToolRegistry::built_in();
        let input = json!({"plan_id":"p1", "duration_min":30, "finish":true,
            "questions_count":5, "correct_count":4, "wrong_questions":[]});
        assert!(registry
            .validate_input("record.checkin_plan", "1", &input)
            .is_ok());
    }

    #[test]
    fn listed_tool_serializes_descriptor_and_ownership() {
        let descriptor = plan::descriptor();
        let listed = ListedTool {
            descriptor,
            ownership: ToolOwnership::RustOwned,
        };
        let value = serde_json::to_value(listed).unwrap();
        assert_eq!(value["descriptor"]["name"], "plan.get_today");
        assert_eq!(value["ownership"], "rust-owned");
    }
}
