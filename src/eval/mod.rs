use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, ensure};
use serde::Deserialize;

use crate::agent::{ChatTurnOutcome, run_chat_turn};
use crate::config::AgentSettings;
use crate::tools::tool_definitions;

pub const DEFAULT_EVAL_CASES_PATH: &str = "eval/cases.yaml";
const DEFAULT_TARGET_PASS_RATE: f64 = 0.80;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalSuite {
    #[serde(default = "default_target_pass_rate")]
    pub target_pass_rate: f64,
    pub cases: Vec<EvalCase>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalCase {
    pub id: String,
    pub prompt: String,
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub answer_format: AnswerFormat,
    #[serde(default)]
    pub answer_must_contain: Vec<String>,
    #[serde(default)]
    pub answer_must_not_contain: Vec<String>,
    #[serde(default)]
    pub no_invented_tool_output: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnswerFormat {
    #[default]
    PlainText,
    JsonObject,
    MarkdownBullets,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalCheckResult {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalCaseResult {
    pub case_id: String,
    pub passed: bool,
    pub checks: Vec<EvalCheckResult>,
    pub error: Option<String>,
    pub final_text: Option<String>,
    pub used_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvalRunReport {
    pub cases_path: PathBuf,
    pub total_cases: usize,
    pub passed_cases: usize,
    pub failed_cases: usize,
    pub pass_rate: f64,
    pub target_pass_rate: f64,
    pub case_results: Vec<EvalCaseResult>,
}

fn default_target_pass_rate() -> f64 {
    DEFAULT_TARGET_PASS_RATE
}

pub fn load_eval_suite(path: &Path) -> Result<EvalSuite> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read eval cases file `{}`", path.display()))?;
    let mut suite = serde_yaml::from_str::<EvalSuite>(&raw)
        .with_context(|| format!("failed to parse eval cases file `{}`", path.display()))?;
    normalize_and_validate_suite(&mut suite)?;
    Ok(suite)
}

pub async fn run_eval_suite(settings: &AgentSettings, cases_path: &Path) -> Result<EvalRunReport> {
    let suite = load_eval_suite(cases_path)?;
    let mut case_results = Vec::with_capacity(suite.cases.len());

    for case in &suite.cases {
        case_results.push(run_eval_case(settings, case).await);
    }

    let passed_cases = case_results.iter().filter(|result| result.passed).count();
    let total_cases = case_results.len();
    let failed_cases = total_cases.saturating_sub(passed_cases);
    let pass_rate = if total_cases == 0 {
        0.0
    } else {
        passed_cases as f64 / total_cases as f64
    };

    Ok(EvalRunReport {
        cases_path: cases_path.to_path_buf(),
        total_cases,
        passed_cases,
        failed_cases,
        pass_rate,
        target_pass_rate: suite.target_pass_rate,
        case_results,
    })
}

pub async fn run_eval_command(settings: &AgentSettings, cases_path: &Path) -> Result<()> {
    let mut eval_settings = settings.clone();
    let eval_notes_dir = create_eval_notes_dir()?;
    eval_settings.notes_dir = eval_notes_dir.display().to_string();

    let report_result = run_eval_suite(&eval_settings, cases_path).await;
    if let Err(error) = fs::remove_dir_all(&eval_notes_dir) {
        eprintln!(
            "warning: failed to remove eval notes directory `{}`: {error}",
            eval_notes_dir.display()
        );
    }
    let report = report_result?;

    println!(
        "Running {} evaluation cases from {}",
        report.total_cases,
        report.cases_path.display()
    );
    for case in &report.case_results {
        if case.passed {
            println!("[PASS] {}", case.case_id);
            continue;
        }

        println!("[FAIL] {}", case.case_id);
        if let Some(error) = &case.error {
            println!("  error: {error}");
        }
        for check in case.checks.iter().filter(|check| !check.passed) {
            println!("  check `{}`: {}", check.name, check.detail);
        }
    }

    let pass_rate_percent = report.pass_rate * 100.0;
    let target_percent = report.target_pass_rate * 100.0;
    println!(
        "Summary: {} passed, {} failed, pass rate {:.1}% (target {:.1}%)",
        report.passed_cases, report.failed_cases, pass_rate_percent, target_percent
    );

    if report.pass_rate + f64::EPSILON < report.target_pass_rate {
        return Err(anyhow!(
            "evaluation pass rate {:.1}% is below target {:.1}%",
            pass_rate_percent,
            target_percent
        ));
    }

    Ok(())
}

fn create_eval_notes_dir() -> Result<PathBuf> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = std::env::temp_dir().join(format!(
        "mjolne_vibes_eval_notes_{}_{}",
        process::id(),
        now_ms
    ));

    fs::create_dir_all(&path)
        .with_context(|| format!("failed to create eval notes directory `{}`", path.display()))?;

    Ok(path)
}

async fn run_eval_case(settings: &AgentSettings, case: &EvalCase) -> EvalCaseResult {
    match run_chat_turn(settings, &case.prompt).await {
        Ok(outcome) => evaluate_case_outcome(case, &outcome),
        Err(error) => EvalCaseResult {
            case_id: case.id.clone(),
            passed: false,
            checks: Vec::new(),
            error: Some(error.to_string()),
            final_text: None,
            used_tools: Vec::new(),
        },
    }
}

fn evaluate_case_outcome(case: &EvalCase, outcome: &ChatTurnOutcome) -> EvalCaseResult {
    let used_tools: Vec<String> = outcome
        .tool_calls
        .iter()
        .map(|call| call.tool_name.clone())
        .collect();

    let checks = vec![
        check_required_tool_usage(case, &used_tools),
        check_no_invented_tool_output(case, outcome),
        check_answer_format(case, &outcome.final_text),
        check_answer_content(case, &outcome.final_text),
    ];
    let passed = checks.iter().all(|check| check.passed);

    EvalCaseResult {
        case_id: case.id.clone(),
        passed,
        checks,
        error: None,
        final_text: Some(outcome.final_text.clone()),
        used_tools,
    }
}

fn check_required_tool_usage(case: &EvalCase, used_tools: &[String]) -> EvalCheckResult {
    if case.required_tools.is_empty() {
        return EvalCheckResult {
            name: "required_tool_usage",
            passed: true,
            detail: "no required tools configured".to_owned(),
        };
    }

    let used_tools: HashSet<_> = used_tools.iter().map(|tool| tool.as_str()).collect();
    let missing: Vec<_> = case
        .required_tools
        .iter()
        .filter(|required| !used_tools.contains(required.as_str()))
        .cloned()
        .collect();

    if missing.is_empty() {
        EvalCheckResult {
            name: "required_tool_usage",
            passed: true,
            detail: "all required tools were used".to_owned(),
        }
    } else {
        EvalCheckResult {
            name: "required_tool_usage",
            passed: false,
            detail: format!("missing required tool calls: {}", missing.join(", ")),
        }
    }
}

fn check_no_invented_tool_output(case: &EvalCase, outcome: &ChatTurnOutcome) -> EvalCheckResult {
    if !case.no_invented_tool_output {
        return EvalCheckResult {
            name: "no_invented_tool_output",
            passed: true,
            detail: "grounding check disabled for this case".to_owned(),
        };
    }

    if outcome.tool_calls.is_empty() {
        return EvalCheckResult {
            name: "no_invented_tool_output",
            passed: false,
            detail: "case requires grounded output but no tool calls were executed".to_owned(),
        };
    }

    let mut allowed_corpus = case.prompt.to_ascii_lowercase();
    for call in &outcome.tool_calls {
        allowed_corpus.push('\n');
        allowed_corpus.push_str(&call.output.to_ascii_lowercase());
    }

    let unknown_quoted_fragments: Vec<String> = extract_quoted_fragments(&outcome.final_text)
        .into_iter()
        .filter(|fragment| fragment.chars().count() >= 4)
        .filter(|fragment| !allowed_corpus.contains(&fragment.to_ascii_lowercase()))
        .collect();

    let mut allowed_numbers = extract_numeric_tokens(&case.prompt);
    for call in &outcome.tool_calls {
        allowed_numbers.extend(extract_numeric_tokens(&call.output));
    }

    let unknown_numbers: Vec<String> = extract_numeric_tokens(&outcome.final_text)
        .into_iter()
        .filter(|number| number.len() >= 3)
        .filter(|number| !allowed_numbers.contains(number))
        .collect();

    let unknown_urls: Vec<String> = extract_urls(&outcome.final_text)
        .into_iter()
        .filter(|url| !allowed_corpus.contains(&url.to_ascii_lowercase()))
        .collect();

    if unknown_quoted_fragments.is_empty() && unknown_numbers.is_empty() && unknown_urls.is_empty()
    {
        return EvalCheckResult {
            name: "no_invented_tool_output",
            passed: true,
            detail: "answer appears grounded in prompt/tool output".to_owned(),
        };
    }

    let mut details = Vec::new();
    if !unknown_quoted_fragments.is_empty() {
        details.push(format!(
            "quoted fragments not found in tool outputs: {}",
            unknown_quoted_fragments.join(", ")
        ));
    }
    if !unknown_numbers.is_empty() {
        details.push(format!(
            "numbers not found in tool outputs: {}",
            unknown_numbers.join(", ")
        ));
    }
    if !unknown_urls.is_empty() {
        details.push(format!(
            "urls not found in tool outputs: {}",
            unknown_urls.join(", ")
        ));
    }

    EvalCheckResult {
        name: "no_invented_tool_output",
        passed: false,
        detail: details.join("; "),
    }
}

fn check_answer_format(case: &EvalCase, answer: &str) -> EvalCheckResult {
    let format_name = "answer_format";

    match case.answer_format {
        AnswerFormat::PlainText => {
            if answer.trim().is_empty() {
                EvalCheckResult {
                    name: format_name,
                    passed: false,
                    detail: "answer is empty".to_owned(),
                }
            } else {
                EvalCheckResult {
                    name: format_name,
                    passed: true,
                    detail: "answer is non-empty plain text".to_owned(),
                }
            }
        }
        AnswerFormat::JsonObject => match serde_json::from_str::<serde_json::Value>(answer) {
            Ok(serde_json::Value::Object(_)) => EvalCheckResult {
                name: format_name,
                passed: true,
                detail: "answer parsed as JSON object".to_owned(),
            },
            Ok(_) => EvalCheckResult {
                name: format_name,
                passed: false,
                detail: "answer is JSON but not an object".to_owned(),
            },
            Err(error) => EvalCheckResult {
                name: format_name,
                passed: false,
                detail: format!("answer is not valid JSON object: {error}"),
            },
        },
        AnswerFormat::MarkdownBullets => {
            let lines: Vec<_> = answer
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();
            if lines.is_empty() {
                return EvalCheckResult {
                    name: format_name,
                    passed: false,
                    detail: "answer is empty".to_owned(),
                };
            }

            let invalid: Vec<_> = lines
                .iter()
                .filter(|line| !line.trim_start().starts_with("- "))
                .map(|line| line.trim().to_owned())
                .collect();

            if invalid.is_empty() {
                EvalCheckResult {
                    name: format_name,
                    passed: true,
                    detail: "answer uses markdown bullet lines".to_owned(),
                }
            } else {
                EvalCheckResult {
                    name: format_name,
                    passed: false,
                    detail: format!("non-bullet lines detected: {}", invalid.join(" | ")),
                }
            }
        }
    }
}

fn check_answer_content(case: &EvalCase, answer: &str) -> EvalCheckResult {
    let normalized_answer = answer.to_ascii_lowercase();
    let missing_required: Vec<String> = case
        .answer_must_contain
        .iter()
        .filter(|expected| !normalized_answer.contains(&expected.to_ascii_lowercase()))
        .cloned()
        .collect();

    let forbidden_found: Vec<String> = case
        .answer_must_not_contain
        .iter()
        .filter(|forbidden| normalized_answer.contains(&forbidden.to_ascii_lowercase()))
        .cloned()
        .collect();

    if missing_required.is_empty() && forbidden_found.is_empty() {
        EvalCheckResult {
            name: "answer_content",
            passed: true,
            detail: "required/forbidden content checks passed".to_owned(),
        }
    } else {
        let mut details = Vec::new();
        if !missing_required.is_empty() {
            details.push(format!(
                "missing required strings: {}",
                missing_required.join(", ")
            ));
        }
        if !forbidden_found.is_empty() {
            details.push(format!(
                "forbidden strings found: {}",
                forbidden_found.join(", ")
            ));
        }
        EvalCheckResult {
            name: "answer_content",
            passed: false,
            detail: details.join("; "),
        }
    }
}

fn normalize_and_validate_suite(suite: &mut EvalSuite) -> Result<()> {
    ensure!(
        (0.0..=1.0).contains(&suite.target_pass_rate),
        "target_pass_rate must be between 0.0 and 1.0"
    );
    ensure!(
        !suite.cases.is_empty(),
        "eval suite must contain at least one case"
    );

    let known_tools: HashSet<&str> = tool_definitions().iter().map(|tool| tool.name).collect();
    let mut ids = HashSet::new();

    for case in &mut suite.cases {
        case.id = case.id.trim().to_owned();
        case.prompt = case.prompt.trim().to_owned();
        ensure!(!case.id.is_empty(), "case id cannot be empty");
        ensure!(!case.prompt.is_empty(), "case prompt cannot be empty");
        ensure!(
            ids.insert(case.id.clone()),
            "duplicate case id `{}`",
            case.id
        );

        case.required_tools = case
            .required_tools
            .iter()
            .map(|tool| tool.trim().to_owned())
            .filter(|tool| !tool.is_empty())
            .collect();
        case.required_tools.sort();
        case.required_tools.dedup();

        for tool in &case.required_tools {
            ensure!(
                known_tools.contains(tool.as_str()),
                "case `{}` references unknown required tool `{tool}`",
                case.id
            );
        }
    }

    Ok(())
}

fn extract_quoted_fragments(text: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut current = String::new();
    let mut quote_char: Option<char> = None;

    for ch in text.chars() {
        match quote_char {
            Some(active) if ch == active => {
                let fragment = current.trim();
                if !fragment.is_empty() {
                    output.push(fragment.to_owned());
                }
                current.clear();
                quote_char = None;
            }
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote_char = Some(ch);
                current.clear();
            }
            None => {}
        }
    }

    output
}

fn extract_numeric_tokens(text: &str) -> BTreeSet<String> {
    let mut output = BTreeSet::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_ascii_digit() || (ch == '.' && !current.is_empty() && !current.contains('.')) {
            current.push(ch);
        } else if !current.is_empty() {
            output.insert(current.clone());
            current.clear();
        }
    }

    if !current.is_empty() {
        output.insert(current);
    }

    output
}

fn extract_urls(text: &str) -> BTreeSet<String> {
    text.split_whitespace()
        .filter(|token| token.starts_with("http://") || token.starts_with("https://"))
        .map(trim_url_token)
        .filter(|token| !token.is_empty())
        .collect()
}

fn trim_url_token(token: &str) -> String {
    let leading_trimmed = token.trim_start_matches(|ch: char| {
        ch == '"' || ch == '\'' || ch == '(' || ch == '[' || ch == '{'
    });
    let trailing_trimmed = leading_trimmed.trim_end_matches(|ch: char| {
        ch == '"'
            || ch == '\''
            || ch == ')'
            || ch == ']'
            || ch == '}'
            || ch == ','
            || ch == '.'
            || ch == ';'
            || ch == ':'
            || ch == '!'
            || ch == '?'
    });

    trailing_trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        AnswerFormat, EvalCase, EvalSuite, check_answer_content, check_answer_format,
        check_no_invented_tool_output, check_required_tool_usage, create_eval_notes_dir,
        extract_numeric_tokens, extract_quoted_fragments, extract_urls,
        normalize_and_validate_suite,
    };
    use crate::agent::{ChatTurnOutcome, ExecutedToolCall, TurnTraceSummary};

    #[test]
    fn normalize_and_validate_suite_rejects_unknown_required_tool() {
        let mut suite = EvalSuite {
            target_pass_rate: 0.8,
            cases: vec![EvalCase {
                id: "case-1".to_owned(),
                prompt: "hello".to_owned(),
                required_tools: vec!["not_a_tool".to_owned()],
                answer_format: AnswerFormat::PlainText,
                answer_must_contain: Vec::new(),
                answer_must_not_contain: Vec::new(),
                no_invented_tool_output: false,
            }],
        };

        let error = normalize_and_validate_suite(&mut suite).expect_err("unknown tool should fail");
        assert!(error.to_string().contains("unknown required tool"));
    }

    #[test]
    fn required_tool_usage_fails_when_missing() {
        let case = EvalCase {
            id: "case-1".to_owned(),
            prompt: "hello".to_owned(),
            required_tools: vec!["fetch_url".to_owned()],
            answer_format: AnswerFormat::PlainText,
            answer_must_contain: Vec::new(),
            answer_must_not_contain: Vec::new(),
            no_invented_tool_output: false,
        };
        let result = check_required_tool_usage(&case, &[]);
        assert!(!result.passed);
    }

    #[test]
    fn no_invented_tool_output_passes_when_answer_is_grounded() {
        let case = EvalCase {
            id: "case-1".to_owned(),
            prompt: "Use fetch_url and summarize example.com".to_owned(),
            required_tools: vec!["fetch_url".to_owned()],
            answer_format: AnswerFormat::PlainText,
            answer_must_contain: Vec::new(),
            answer_must_not_contain: Vec::new(),
            no_invented_tool_output: true,
        };
        let outcome = test_outcome(
            "The page title is \"Example Domain\".",
            vec![(
                "fetch_url",
                r#"{"url":"https://example.com","content":"Example Domain"}"#,
            )],
        );

        let result = check_no_invented_tool_output(&case, &outcome);
        assert!(result.passed, "{}", result.detail);
    }

    #[test]
    fn no_invented_tool_output_fails_on_unseen_number() {
        let case = EvalCase {
            id: "case-1".to_owned(),
            prompt: "Use fetch_url on example.com".to_owned(),
            required_tools: vec!["fetch_url".to_owned()],
            answer_format: AnswerFormat::PlainText,
            answer_must_contain: Vec::new(),
            answer_must_not_contain: Vec::new(),
            no_invented_tool_output: true,
        };
        let outcome = test_outcome(
            "Status was 404 and title was Example Domain.",
            vec![(
                "fetch_url",
                r#"{"status_code":200,"content":"Example Domain"}"#,
            )],
        );

        let result = check_no_invented_tool_output(&case, &outcome);
        assert!(!result.passed);
        assert!(result.detail.contains("numbers not found"));
    }

    #[test]
    fn answer_format_json_object_requires_json_object() {
        let case = EvalCase {
            id: "case-1".to_owned(),
            prompt: "Respond with JSON".to_owned(),
            required_tools: Vec::new(),
            answer_format: AnswerFormat::JsonObject,
            answer_must_contain: Vec::new(),
            answer_must_not_contain: Vec::new(),
            no_invented_tool_output: false,
        };

        let result = check_answer_format(&case, r#"{"ok":true}"#);
        assert!(result.passed);

        let result = check_answer_format(&case, "not-json");
        assert!(!result.passed);
    }

    #[test]
    fn answer_content_checks_required_and_forbidden_strings() {
        let case = EvalCase {
            id: "case-1".to_owned(),
            prompt: "hello".to_owned(),
            required_tools: Vec::new(),
            answer_format: AnswerFormat::PlainText,
            answer_must_contain: vec!["rust".to_owned()],
            answer_must_not_contain: vec!["python".to_owned()],
            no_invented_tool_output: false,
        };

        let result = check_answer_content(&case, "Rust only");
        assert!(result.passed);

        let result = check_answer_content(&case, "python and rust");
        assert!(!result.passed);
    }

    #[test]
    fn extract_helpers_capture_expected_values() {
        let numbers = extract_numeric_tokens("Status 200 and 12.5 ms");
        assert!(numbers.contains("200"));
        assert!(numbers.contains("12.5"));

        let quotes = extract_quoted_fragments("title \"Example Domain\"");
        assert_eq!(quotes, vec!["Example Domain".to_owned()]);

        let urls = extract_urls("see https://example.com/test, now");
        assert!(urls.contains("https://example.com/test"));
    }

    #[test]
    fn create_eval_notes_dir_creates_unique_temp_directory() {
        let path = create_eval_notes_dir().expect("eval notes dir should be created");
        assert!(path.exists());
        assert!(path.is_dir());
        assert!(
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("mjolne_vibes_eval_notes_"))
                .unwrap_or(false)
        );
        std::fs::remove_dir_all(path).expect("temp eval dir cleanup should succeed");
    }

    fn test_outcome(final_text: &str, tool_calls: Vec<(&str, &str)>) -> ChatTurnOutcome {
        ChatTurnOutcome {
            final_text: final_text.to_owned(),
            trace: TurnTraceSummary {
                input_chars: 0,
                output_chars: Some(final_text.chars().count()),
                steps_executed: 1,
                model_calls: 1,
                tool_calls: tool_calls.len() as u32,
                total_model_latency: Duration::from_millis(1),
                total_tool_latency: Duration::from_millis(1),
                tool_names: tool_calls
                    .iter()
                    .map(|(name, _)| (*name).to_owned())
                    .collect(),
            },
            tool_calls: tool_calls
                .into_iter()
                .map(|(tool_name, output)| ExecutedToolCall {
                    tool_name: tool_name.to_owned(),
                    output: output.to_owned(),
                })
                .collect(),
        }
    }
}
