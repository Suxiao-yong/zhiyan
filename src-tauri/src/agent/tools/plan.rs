use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Confirmation, Idempotency, RiskLevel, ToolDescriptor};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanGetTodayInput {
    pub exam_id: String,
}

pub fn descriptor() -> ToolDescriptor {
    let plan_fields = json!({
        "id": {"type":"string"}, "exam_id": {"type":"string"}, "subject_id": {"type":"string"},
        "knowledge_point_id": {"type":["string","null"]}, "date": {"type":"string"},
        "planned_tasks": {"type":["string","null"]}, "planned_duration": {"type":["integer","null"]},
        "actual_duration": {"type":["integer","null"]}, "actual_tasks": {"type":["string","null"]},
        "status": {"type":"string"}, "generated_by": {"type":"string"},
        "ai_suggestion": {"type":["string","null"]}, "user_modified": {"type":"integer"},
        "sort_order": {"type":"integer"}, "created_at": {"type":"string"}, "updated_at": {"type":"string"},
        "subject_name": {"type":["string","null"]}, "knowledge_point_name": {"type":["string","null"]},
        "record_count": {"type":"integer"}
    });
    ToolDescriptor {
        name: "plan.get_today",
        version: "1",
        input_schema: json!({"type":"object", "additionalProperties":false, "required":["exam_id"], "properties":{"exam_id":{"type":"string","minLength":1}}}),
        output_schema: json!({"type":"object", "required":["business_date","plans"], "properties":{"business_date":{"type":"string"},"plans":{"type":"array","items":{"type":"object","properties":plan_fields["properties"].clone()}}}}),
        risk: RiskLevel::R0,
        confirmation: Confirmation::Automatic,
        supports_undo: false,
        timeout_ms: 2000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec![
            "study_plans:read",
            "subjects:read",
            "knowledge_points:read",
            "study_records:aggregate",
        ],
    }
}
