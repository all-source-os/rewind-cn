use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::domain::error::RewindError;
use crate::domain::events::AcceptanceCriterion;
use crate::infrastructure::coder::ToolCallRecord;
use crate::infrastructure::llm::{AgentConfig, ProviderClient};

const MAX_PARSE_RETRIES: usize = 2;

/// Result of evaluating a single acceptance criterion.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CriterionResult {
    /// 0-based index into the acceptance criteria list.
    pub index: usize,
    /// Whether this criterion was satisfied.
    pub passed: bool,
    /// Explanation of why it passed or failed.
    pub reason: String,
}

/// The evaluator's overall judgment of task completion.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvaluationResult {
    /// Whether all criteria are satisfied.
    pub passed: bool,
    /// Per-criterion results.
    pub criteria_results: Vec<CriterionResult>,
    /// Overall summary of the evaluation.
    pub summary: String,
}

const EVALUATOR_SYSTEM_PROMPT: &str = r#"You are a code review evaluator. Your job is to judge whether an AI coding agent has successfully completed a task by verifying each acceptance criterion.

You will be given:
1. The task description and acceptance criteria
2. A log of tool calls the agent made (files read/written, commands run)
3. The agent's final output

## Output Format
Return ONLY a JSON object with this exact structure (no markdown, no code fences):

{
  "passed": true/false,
  "criteria_results": [
    {
      "index": 0,
      "passed": true/false,
      "reason": "Explanation of why this criterion passed or failed"
    }
  ],
  "summary": "Overall summary of the evaluation"
}

## Rules
1. Be strict but fair. If the agent claims it did something but the tool log doesn't confirm it, mark it as failed.
2. If a command returned exit code 0, it generally passed.
3. If a file was written with the expected content, the criterion is likely met.
4. If there's no evidence for a criterion (no relevant tool calls), mark it as failed with reason "No evidence found".
5. The "passed" top-level field should be true ONLY if ALL criteria passed.
"#;

/// Strip markdown code fences from LLM response and parse as JSON.
fn strip_fences_and_parse(response: &str) -> Result<EvaluationResult, serde_json::Error> {
    let trimmed = response.trim();
    let json_str = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s: &str| s.strip_suffix("```"))
        .unwrap_or(trimmed);
    serde_json::from_str::<EvaluationResult>(json_str)
}

/// Build a corrective prompt that includes the original input and the parse error.
fn corrective_prompt(original_input: &str, error: &serde_json::Error) -> String {
    format!(
        "{original_input}\n\n\
        Your previous response was not valid JSON. \
        The parse error was: {error}\n\
        Please respond with valid JSON only."
    )
}

/// LLM-powered evaluator agent that judges task completion.
pub struct EvaluatorAgent {
    client: ProviderClient,
    config: AgentConfig,
}

impl EvaluatorAgent {
    pub fn new(client: ProviderClient, config: AgentConfig) -> Self {
        Self { client, config }
    }

    /// Evaluate whether the coder agent successfully completed the task.
    #[hotpath::measure]
    pub async fn evaluate(
        &self,
        task_description: &str,
        acceptance_criteria: &[AcceptanceCriterion],
        tool_calls: &[ToolCallRecord],
        agent_output: &str,
    ) -> Result<EvaluationResult, RewindError> {
        let eval_input = build_eval_input(
            task_description,
            acceptance_criteria,
            tool_calls,
            agent_output,
        );

        let mut user_prompt = eval_input.clone();
        let mut last_error: Option<serde_json::Error> = None;
        let mut last_response = String::new();

        for attempt in 0..=MAX_PARSE_RETRIES {
            let response: String = self
                .client
                .prompt(
                    &self.config.evaluator.model,
                    EVALUATOR_SYSTEM_PROMPT,
                    self.config.evaluator.max_tokens as u64,
                    &user_prompt,
                )
                .await?;

            match strip_fences_and_parse(&response) {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_response = response;
                    if attempt < MAX_PARSE_RETRIES {
                        warn!(
                            attempt = attempt + 1,
                            max_retries = MAX_PARSE_RETRIES,
                            error = %e,
                            "Evaluator returned invalid JSON, retrying with corrective prompt"
                        );
                        user_prompt = corrective_prompt(&eval_input, &e);
                    }
                    last_error = Some(e);
                }
            }
        }

        Err(RewindError::Config(format!(
            "Failed to parse evaluator output as EvaluationResult after {MAX_PARSE_RETRIES} retries: {}\n\nRaw output:\n{last_response}",
            last_error.unwrap(),
        )))
    }
}

/// Build the evaluation input prompt from task context and agent results.
fn build_eval_input(
    task_description: &str,
    acceptance_criteria: &[AcceptanceCriterion],
    tool_calls: &[ToolCallRecord],
    agent_output: &str,
) -> String {
    let mut input = format!("## Task Description\n{task_description}\n\n## Acceptance Criteria\n");

    for (i, criterion) in acceptance_criteria.iter().enumerate() {
        input.push_str(&format!("{}. {}\n", i, criterion.description));
    }

    input.push_str("\n## Agent Tool Call Log\n");
    if tool_calls.is_empty() {
        input.push_str("(no tool calls recorded)\n");
    } else {
        for call in tool_calls {
            input.push_str(&format!(
                "- **{}**({}): {}\n",
                call.tool_name, call.args_summary, call.result_summary
            ));
        }
    }

    input.push_str(&format!("\n## Agent Final Output\n{agent_output}\n"));
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_EVAL_JSON: &str = r#"{"passed":true,"criteria_results":[{"index":0,"passed":true,"reason":"Done"}],"summary":"All good"}"#;

    #[test]
    fn evaluation_result_deserializes() {
        let json = r#"{
            "passed": false,
            "criteria_results": [
                {"index": 0, "passed": true, "reason": "File was created successfully"},
                {"index": 1, "passed": false, "reason": "Tests were not run"}
            ],
            "summary": "1 of 2 criteria met. Tests were not executed."
        }"#;

        let result: EvaluationResult = serde_json::from_str(json).unwrap();
        assert!(!result.passed);
        assert_eq!(result.criteria_results.len(), 2);
        assert!(result.criteria_results[0].passed);
        assert!(!result.criteria_results[1].passed);
        assert!(result.summary.contains("1 of 2"));
    }

    #[test]
    fn evaluation_result_all_passed() {
        let json = r#"{
            "passed": true,
            "criteria_results": [
                {"index": 0, "passed": true, "reason": "Migration file exists and is valid"},
                {"index": 1, "passed": true, "reason": "cargo test exited with code 0"}
            ],
            "summary": "All criteria met."
        }"#;

        let result: EvaluationResult = serde_json::from_str(json).unwrap();
        assert!(result.passed);
        assert!(result.criteria_results.iter().all(|c| c.passed));
    }

    #[test]
    fn build_eval_input_includes_all_sections() {
        let criteria = vec![
            AcceptanceCriterion {
                description: "File exists".into(),
                checked: false,
            },
            AcceptanceCriterion {
                description: "Tests pass".into(),
                checked: false,
            },
        ];

        let tool_calls = vec![
            ToolCallRecord {
                tool_name: "write_file".into(),
                args_summary: "src/main.rs".into(),
                result_summary: "Wrote 100 bytes".into(),
            },
            ToolCallRecord {
                tool_name: "run_command".into(),
                args_summary: "cargo test".into(),
                result_summary: "exit 0, 200 bytes output".into(),
            },
        ];

        let input = build_eval_input("Create main.rs", &criteria, &tool_calls, "Done!");

        assert!(input.contains("Create main.rs"));
        assert!(input.contains("File exists"));
        assert!(input.contains("Tests pass"));
        assert!(input.contains("write_file"));
        assert!(input.contains("run_command"));
        assert!(input.contains("Done!"));
    }

    #[test]
    fn build_eval_input_handles_empty_tool_calls() {
        let input = build_eval_input("Task", &[], &[], "output");
        assert!(input.contains("no tool calls recorded"));
    }

    #[test]
    fn strip_fences_and_parse_valid_json() {
        let result = strip_fences_and_parse(VALID_EVAL_JSON).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn strip_fences_and_parse_with_fences() {
        let fenced = format!("```json\n{}\n```", VALID_EVAL_JSON);
        let result = strip_fences_and_parse(&fenced).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn strip_fences_and_parse_invalid_json() {
        assert!(strip_fences_and_parse("not json at all").is_err());
    }

    #[test]
    fn corrective_prompt_contains_error_info() {
        let err = strip_fences_and_parse("bad").unwrap_err();
        let prompt = corrective_prompt("original input", &err);
        assert!(prompt.starts_with("original input"));
        assert!(prompt.contains("not valid JSON"));
    }

    #[tokio::test]
    async fn evaluator_retry_succeeds_on_second_attempt() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();
        let valid = VALID_EVAL_JSON.to_string();

        let client = ProviderClient::Mock(Arc::new(move |_, _, _, _| {
            let n = cc.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                "not json".into()
            } else {
                valid.clone()
            }
        }));

        let config = AgentConfig::default();
        let agent = EvaluatorAgent::new(client, config);

        let result = agent.evaluate("desc", &[], &[], "output").await;
        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn evaluator_fails_after_max_retries() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();

        let client = ProviderClient::Mock(Arc::new(move |_, _, _, _| {
            cc.fetch_add(1, Ordering::SeqCst);
            "never valid".into()
        }));

        let config = AgentConfig::default();
        let agent = EvaluatorAgent::new(client, config);

        let result = agent.evaluate("desc", &[], &[], "output").await;
        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 3); // 1 initial + 2 retries
    }

    #[tokio::test]
    async fn evaluator_no_retry_on_first_success() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();
        let valid = VALID_EVAL_JSON.to_string();

        let client = ProviderClient::Mock(Arc::new(move |_, _, _, _| {
            cc.fetch_add(1, Ordering::SeqCst);
            valid.clone()
        }));

        let config = AgentConfig::default();
        let agent = EvaluatorAgent::new(client, config);

        let result = agent.evaluate("desc", &[], &[], "output").await;
        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }
}
