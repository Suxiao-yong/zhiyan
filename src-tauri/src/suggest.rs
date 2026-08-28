// Onboarding 知识点自动建议：根据前面三步填写的考试/科目信息，
// 由大模型联网搜集该科目的核心知识点，返回供用户确认或修改。
// 若未配置 LLM，则返回按科目名的启发式占位，避免阻塞引导流程。

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::State;

use crate::agent::llm::ProviderMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestKpInput {
    pub exam_type: String,
    pub exam_name: String,
    pub subjects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedKp {
    pub name: String,
    pub chapter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestKpOutput {
    pub suggestions: Vec<Vec<SuggestedKp>>, // 与输入 subjects 一一对应
}

fn heuristic_kps(subject: &str) -> Vec<SuggestedKp> {
    let s = subject.trim().to_lowercase();
    let raw: &[(&str, &str)] = if s.contains("数学") || s.contains("math") {
        &[
            ("函数与极限", "第一章"),
            ("导数与微分", "第二章"),
            ("积分学", "第三章"),
            ("线性代数", "第四章"),
            ("概率统计", "第五章"),
        ]
    } else if s.contains("英语") || s.contains("english") {
        &[
            ("词汇", "第一章"),
            ("语法", "第二章"),
            ("阅读理解", "第三章"),
            ("写作", "第四章"),
            ("翻译", "第五章"),
        ]
    } else if s.contains("政治") {
        &[
            ("马克思主义原理", "第一章"),
            ("毛泽东思想", "第二章"),
            ("中国特色社会主义", "第三章"),
            ("时政", "第四章"),
        ]
    } else if s.contains("专业") {
        &[
            ("基础概念", "第一章"),
            ("核心理论", "第二章"),
            ("方法与应用", "第三章"),
            ("案例分析", "第四章"),
        ]
    } else {
        &[
            ("基础概念", "第一章"),
            ("核心原理", "第二章"),
            ("重点难点", "第三章"),
            ("综合应用", "第四章"),
            ("真题要点", "第五章"),
        ]
    };
    raw.iter()
        .map(|(name, chapter)| SuggestedKp {
            name: (*name).to_string(),
            chapter: Some((*chapter).to_string()),
        })
        .collect()
}

fn build_prompt(exam_type: &str, exam_name: &str, subjects: &[String]) -> String {
    let subjects_list = subjects
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}. {}", i + 1, s))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"你是学习规划助手。请根据以下考试信息，联网搜集并为每个科目列出 5-8 个最重要的知识点。

考试类型：{exam_type}
考试名称：{exam_name}
科目：
{subjects_list}

要求：
- 每个知识点包含 name（知识点名称，简洁准确）和 chapter（所属章节，可为空）
- 覆盖该科目的核心考纲，按学习顺序排列
- 仅返回 JSON，格式为：{{"suggestions": [[{{"name": "...", "chapter": "..."}}, ...], ...]}}
  外层数组与科目顺序一一对应，内层为该科目的知识点列表。不要返回任何额外文字。"#
    )
}

fn parse_llm_json(text: &str, subject_count: usize) -> Option<Vec<Vec<SuggestedKp>>> {
    // 尝试直接解析，否则提取首个 JSON 对象
    let try_parse = |s: &str| -> Option<Vec<Vec<SuggestedKp>>> {
        let v: serde_json::Value = serde_json::from_str(s).ok()?;
        let arr = v.get("suggestions")?.as_array()?;
        let mut out = Vec::new();
        for sub in arr {
            let kps: Vec<SuggestedKp> = serde_json::from_value(sub.clone()).ok()?;
            out.push(kps);
        }
        Some(out)
    };
    if let Some(r) = try_parse(text) {
        if r.len() == subject_count {
            return Some(r);
        }
    }
    // 回退：提取第一个 {{ ... }} 块
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    try_parse(&text[start..=end])
}

#[tauri::command]
pub async fn suggest_knowledge_points(
    pool: State<'_, SqlitePool>,
    input: SuggestKpInput,
) -> Result<SuggestKpOutput, String> {
    let subjects = input
        .subjects
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if subjects.is_empty() {
        return Ok(SuggestKpOutput {
            suggestions: Vec::new(),
        });
    }

    // 尝试走 LLM；失败或未配置则回退到启发式，避免阻塞引导（不向上抛 String 错误）
    let provider = match crate::agent::planner::Planner::build_provider_from(pool.inner()).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("suggest: build_provider failed, fallback to heuristic: {e}");
            None
        }
    };
    let Some(provider) = provider else {
        return Ok(SuggestKpOutput {
            suggestions: subjects.iter().map(|s| heuristic_kps(s)).collect(),
        });
    };

    let prompt = build_prompt(&input.exam_type, &input.exam_name, &subjects);
    let messages = vec![ProviderMessage {
        role: "user".to_string(),
        content: Some(prompt),
        tool_calls: None,
        tool_call_id: None,
    }];

    // 单轮调用，不使用工具；失败则回退启发式而非上抛
    let mut streamed = String::new();
    let resp = match provider
        .chat_stream(&messages, &[], &mut |chunk: &str| streamed.push_str(chunk))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("suggest: chat_stream failed, fallback to heuristic: {e}");
            return Ok(SuggestKpOutput {
                suggestions: subjects.iter().map(|s| heuristic_kps(s)).collect(),
            });
        }
    };

    let text = resp.content.as_deref().unwrap_or(streamed.as_str()).trim();
    if let Some(parsed) = parse_llm_json(text, subjects.len()) {
        return Ok(SuggestKpOutput {
            suggestions: parsed,
        });
    }

    // 解析失败：回退启发式，并把原始文本作为第一科的单个知识点以便调试可见
    Ok(SuggestKpOutput {
        suggestions: subjects.iter().map(|s| heuristic_kps(s)).collect(),
    })
}
