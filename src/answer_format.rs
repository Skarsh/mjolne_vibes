use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredAnswerFormat {
    JsonObject,
    MarkdownBullets,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuredAnswerFormatError {
    EmptyAnswer,
    JsonNotObject,
    JsonParseError(String),
    NonBulletLines(Vec<String>),
}

pub fn answer_matches_structured_format(format: StructuredAnswerFormat, answer: &str) -> bool {
    validate_structured_answer_format(format, answer).is_ok()
}

pub fn validate_structured_answer_format(
    format: StructuredAnswerFormat,
    answer: &str,
) -> Result<(), StructuredAnswerFormatError> {
    match format {
        StructuredAnswerFormat::JsonObject => validate_json_object(answer),
        StructuredAnswerFormat::MarkdownBullets => validate_markdown_bullets(answer),
    }
}

fn validate_json_object(answer: &str) -> Result<(), StructuredAnswerFormatError> {
    match serde_json::from_str::<Value>(answer) {
        Ok(Value::Object(_)) => Ok(()),
        Ok(_) => Err(StructuredAnswerFormatError::JsonNotObject),
        Err(error) => Err(StructuredAnswerFormatError::JsonParseError(
            error.to_string(),
        )),
    }
}

fn validate_markdown_bullets(answer: &str) -> Result<(), StructuredAnswerFormatError> {
    let lines: Vec<_> = answer
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return Err(StructuredAnswerFormatError::EmptyAnswer);
    }

    let invalid: Vec<String> = lines
        .iter()
        .filter(|line| !line.trim_start().starts_with("- "))
        .map(|line| line.trim().to_owned())
        .collect();

    if invalid.is_empty() {
        Ok(())
    } else {
        Err(StructuredAnswerFormatError::NonBulletLines(invalid))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        StructuredAnswerFormat, StructuredAnswerFormatError, answer_matches_structured_format,
        validate_structured_answer_format,
    };

    #[test]
    fn json_object_validation_accepts_objects() {
        assert!(answer_matches_structured_format(
            StructuredAnswerFormat::JsonObject,
            r#"{"ok":true}"#
        ));
    }

    #[test]
    fn json_object_validation_rejects_non_objects() {
        let error =
            validate_structured_answer_format(StructuredAnswerFormat::JsonObject, "[1,2,3]")
                .expect_err("non-object json should fail");
        assert_eq!(error, StructuredAnswerFormatError::JsonNotObject);
    }

    #[test]
    fn json_object_validation_rejects_invalid_json() {
        let error =
            validate_structured_answer_format(StructuredAnswerFormat::JsonObject, "not-json")
                .expect_err("invalid json should fail");
        let StructuredAnswerFormatError::JsonParseError(message) = error else {
            panic!("expected json parse error");
        };
        assert!(!message.is_empty());
    }

    #[test]
    fn markdown_bullets_validation_accepts_bullets() {
        assert!(answer_matches_structured_format(
            StructuredAnswerFormat::MarkdownBullets,
            "- one\n- two"
        ));
    }

    #[test]
    fn markdown_bullets_validation_rejects_empty_answer() {
        let error =
            validate_structured_answer_format(StructuredAnswerFormat::MarkdownBullets, "  \n \n")
                .expect_err("empty answer should fail");
        assert_eq!(error, StructuredAnswerFormatError::EmptyAnswer);
    }

    #[test]
    fn markdown_bullets_validation_reports_non_bullet_lines() {
        let error = validate_structured_answer_format(
            StructuredAnswerFormat::MarkdownBullets,
            "- one\nnot bullet\n- two",
        )
        .expect_err("non-bullet lines should fail");
        let StructuredAnswerFormatError::NonBulletLines(lines) = error else {
            panic!("expected non-bullet line error");
        };
        assert_eq!(lines, vec!["not bullet".to_owned()]);
    }
}
