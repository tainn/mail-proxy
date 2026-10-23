use std::collections::BTreeMap;

use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use lettre::message::header::ContentType;
use lettre::message::{Mailbox, Mailboxes};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendRequest {
    pub from: Option<Mailbox>,
    pub to: Mailboxes,
    #[serde(default)]
    pub cc: Mailboxes,
    #[serde(default)]
    pub bcc: Mailboxes,
    #[serde(default)]
    pub reply_to: Mailboxes,
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub format: BodyFormat,
    pub text_body: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyFormat {
    #[default]
    Markdown,
    Html,
    Text,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub filename: String,
    #[serde(deserialize_with = "base64")]
    pub content: Vec<u8>,
    pub content_type: Option<ContentType>,
    pub content_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SendResponse {
    pub message_id: String,
}

fn base64<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let mut encoded = String::deserialize(deserializer)?;
    encoded.retain(|char| !char.is_ascii_whitespace());
    BASE64
        .decode(encoded)
        .map_err(|error| D::Error::custom(format!("invalid base64: {error}")))
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use serde_json::json;

    use super::*;

    fn addresses(mailboxes: &Mailboxes) -> Vec<String> {
        mailboxes.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parses_minimal_request() -> Result<(), Box<dyn Error>> {
        let request: SendRequest = serde_json::from_value(json!({
            "to": "foo@domain.tld",
            "subject": "Hi",
            "body": "**Hello**",
        }))?;
        assert!(request.from.is_none());
        assert_eq!(addresses(&request.to), ["foo@domain.tld"]);
        assert_eq!(addresses(&request.cc), Vec::<String>::new());
        assert_eq!(request.format, BodyFormat::Markdown);
        assert!(request.attachments.is_empty());
        assert!(request.headers.is_empty());
        Ok(())
    }

    #[test]
    fn parses_mailbox_variants() -> Result<(), Box<dyn Error>> {
        let request: SendRequest = serde_json::from_value(json!({
            "from": { "name": "Foo", "email": "foo@domain.tld" },
            "to": ["Bar <bar@domain.tld>", { "email": "baz@domain.tld" }],
            "cc": "qux@domain.tld, Quux <quux@domain.tld>",
            "subject": "Hi",
            "body": "Hello",
            "format": "text",
        }))?;
        let from = request.from.map(|from| from.to_string());
        assert_eq!(from.as_deref(), Some("Foo <foo@domain.tld>"));
        assert_eq!(
            addresses(&request.to),
            ["Bar <bar@domain.tld>", "baz@domain.tld"]
        );
        assert_eq!(
            addresses(&request.cc),
            ["qux@domain.tld", "Quux <quux@domain.tld>"]
        );
        assert_eq!(request.format, BodyFormat::Text);
        Ok(())
    }

    #[test]
    fn decodes_lenient_base64() -> Result<(), Box<dyn Error>> {
        let request: SendRequest = serde_json::from_value(json!({
            "to": "foo@domain.tld",
            "subject": "Hi",
            "body": "Hello",
            "attachments": [{ "filename": "a.txt", "content": "aGVs\nbG8h" }],
        }))?;
        let content = request.attachments.first().map(|a| a.content.as_slice());
        assert_eq!(content, Some(b"hello!".as_slice()));
        Ok(())
    }

    #[test]
    fn rejects_invalid_input() {
        let cases = [
            json!({ "subject": "Hi", "body": "Hello" }),
            json!({ "to": "foo@domain.tld", "body": "Hello" }),
            json!({ "to": "foo@domain.tld", "subject": "Hi" }),
            json!({ "to": "not an address", "subject": "Hi", "body": "Hello" }),
            json!({ "to": "foo@domain.tld", "subject": "Hi", "body": "Hello", "html": "x" }),
            json!({ "to": "foo@domain.tld", "subject": "Hi", "body": "Hello", "format": "rtf" }),
            json!({
                "to": "foo@domain.tld",
                "subject": "Hi",
                "body": "Hello",
                "attachments": [{ "filename": "a.txt", "content": "not base64!" }],
            }),
            json!({
                "to": "foo@domain.tld",
                "subject": "Hi",
                "body": "Hello",
                "attachments": [{ "filename": "a", "content": "", "content_type": "nope" }],
            }),
        ];
        for case in cases {
            let result = SendRequest::deserialize(&case);
            assert!(result.is_err(), "{case} should be rejected");
        }
    }
}
