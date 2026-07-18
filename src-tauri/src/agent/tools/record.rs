use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Confirmation, Idempotency, RiskLevel, ToolDescriptor};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckinWrongQuestionInput {
    pub question_source: Option<String>,
    pub question_desc: Option<String>,
    pub correct_answer: Option<String>,
    pub my_answer: Option<String>,
    pub error_type: Option<String>,
    pub error_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordCheckinPlanInput {
    pub plan_id: String,
    pub duration_min: i64,
    pub content: Option<String>,
    #[serde(default)]
    pub questions_count: i64,
    #[serde(default)]
    pub correct_count: i64,
    pub mastery_rating: Option<i64>,
    pub difficulty_notes: Option<String>,
    pub mood: Option<i64>,
    pub session_time: Option<String>,
    #[serde(default)]
    pub finish: bool,
    #[serde(default)]
    pub wrong_questions: Vec<CheckinWrongQuestionInput>,
}

pub fn descriptor() -> ToolDescriptor {
    let wrong = json!({"type":"object", "additionalProperties":false, "properties": {
        "question_source":{"type":["string","null"]}, "question_desc":{"type":["string","null"]},
        "correct_answer":{"type":["string","null"]}, "my_answer":{"type":["string","null"]},
        "error_type":{"type":["string","null"]}, "error_reason":{"type":["string","null"]}
    }});
    ToolDescriptor {
        name: "record.checkin_plan",
        version: "1",
        input_schema: json!({"type":"object", "additionalProperties":false, "required":["plan_id","duration_min","finish"], "properties": {
            "plan_id":{"type":"string","minLength":1}, "duration_min":{"type":"integer","minimum":1},
            "content":{"type":["string","null"]}, "questions_count":{"type":"integer","minimum":0,"default":0},
            "correct_count":{"type":"integer","minimum":0,"default":0}, "mastery_rating":{"type":["integer","null"],"minimum":1,"maximum":5},
            "difficulty_notes":{"type":["string","null"]}, "mood":{"type":["integer","null"],"minimum":1,"maximum":5},
            "session_time":{"type":["string","null"],"enum":["morning","afternoon","evening",null]},
            "finish":{"type":"boolean","default":false}, "wrong_questions":{"type":"array","default":[],"items":wrong}
        }}),
        output_schema: json!({"type":"object"}),
        risk: RiskLevel::R1,
        confirmation: Confirmation::Automatic,
        supports_undo: true,
        timeout_ms: 5000,
        idempotency: Idempotency::RequiredExactlyOnce,
        data_permissions: vec![
            "study_plans:read_write",
            "study_records:write",
            "wrong_questions:write",
            "agent_audit:write",
        ],
    }
}
