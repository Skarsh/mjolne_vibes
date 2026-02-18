use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchNotesArgs {
    pub query: String,
    pub limit: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchUrlArgs {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveNoteArgs {
    pub title: String,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use serde::de::DeserializeOwned;
    use serde_json::json;

    use super::{FetchUrlArgs, SaveNoteArgs, SearchNotesArgs};

    fn parse_args<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    #[test]
    fn search_notes_args_parse_valid_payload() {
        let args = parse_args::<SearchNotesArgs>(json!({
            "query": "rust testing",
            "limit": 5
        }))
        .expect("valid search_notes payload should parse");

        assert_eq!(args.query, "rust testing");
        assert_eq!(args.limit, 5);
    }

    #[test]
    fn search_notes_args_reject_unknown_field() {
        let error = parse_args::<SearchNotesArgs>(json!({
            "query": "rust",
            "limit": 3,
            "unexpected": true
        }))
        .expect_err("unknown field should be rejected");

        assert!(error.contains("unknown field"));
        assert!(error.contains("unexpected"));
    }

    #[test]
    fn search_notes_args_reject_invalid_limit_type() {
        let error = parse_args::<SearchNotesArgs>(json!({
            "query": "rust",
            "limit": "3"
        }))
        .expect_err("invalid limit type should be rejected");

        assert!(error.contains("invalid type"));
    }

    #[test]
    fn fetch_url_args_parse_valid_payload() {
        let args = parse_args::<FetchUrlArgs>(json!({
            "url": "https://example.com"
        }))
        .expect("valid fetch_url payload should parse");

        assert_eq!(args.url, "https://example.com");
    }

    #[test]
    fn fetch_url_args_reject_unknown_field() {
        let error = parse_args::<FetchUrlArgs>(json!({
            "url": "https://example.com",
            "method": "GET"
        }))
        .expect_err("unknown field should be rejected");

        assert!(error.contains("unknown field"));
        assert!(error.contains("method"));
    }

    #[test]
    fn save_note_args_parse_valid_payload() {
        let args = parse_args::<SaveNoteArgs>(json!({
            "title": "daily note",
            "body": "shipped phase two task one"
        }))
        .expect("valid save_note payload should parse");

        assert_eq!(args.title, "daily note");
        assert_eq!(args.body, "shipped phase two task one");
    }

    #[test]
    fn save_note_args_reject_unknown_field() {
        let error = parse_args::<SaveNoteArgs>(json!({
            "title": "daily note",
            "body": "content",
            "path": "/tmp/notes.md"
        }))
        .expect_err("unknown field should be rejected");

        assert!(error.contains("unknown field"));
        assert!(error.contains("path"));
    }
}
