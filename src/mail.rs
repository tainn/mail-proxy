use std::collections::BTreeMap;

use lettre::Message;
use lettre::message::header::{
    Bcc, Cc, ContentType, Header, HeaderName, HeaderValue, InvalidHeaderName, ReplyTo, To,
};
use lettre::message::{
    Attachment as AttachmentPart, Mailbox, Mailboxes, MessageBuilder, MultiPart, MultiPartBuilder,
    SinglePart,
};
use uuid::Uuid;

use crate::api::{Attachment, BodyFormat, SendRequest};
use crate::render::{self, Content};

const MAX_HEADER_NAME_LEN: usize = 76;

const RESERVED_HEADERS: [&str; 14] = [
    "bcc",
    "cc",
    "content-disposition",
    "content-id",
    "content-transfer-encoding",
    "content-type",
    "date",
    "from",
    "message-id",
    "mime-version",
    "reply-to",
    "sender",
    "subject",
    "to",
];

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("from is required because MAIL_DEFAULT_FROM is not configured")]
    MissingFrom,
    #[error("to must contain at least one recipient")]
    MissingRecipient,
    #[error("text_body can only be used with the html or markdown format")]
    UnexpectedTextBody,
    #[error("header {0:?} is managed by the proxy and cannot be set")]
    ReservedHeader(String),
    #[error("header name {0:?} is invalid")]
    InvalidHeaderName(String),
    #[error(transparent)]
    HeaderName(#[from] InvalidHeaderName),
    #[error(
        "content_id {0:?} is invalid, use printable ascii characters without whitespace or angle brackets"
    )]
    InvalidContentId(String),
    #[error("failed to derive plain text from html: {0}")]
    PlainText(#[from] html2text::Error),
    #[error("invalid message: {0}")]
    Message(#[from] lettre::error::Error),
}

pub struct Mail {
    pub message_id: String,
    pub message: Message,
}

enum Body {
    Single(SinglePart),
    Multi(MultiPart),
}

pub fn build(request: SendRequest, default_from: Option<&Mailbox>) -> Result<Mail, BuildError> {
    validate(&request)?;
    let SendRequest {
        from,
        to,
        cc,
        bcc,
        reply_to,
        subject,
        body,
        format,
        text_body,
        attachments,
        headers,
    } = request;

    let from = from
        .or_else(|| default_from.cloned())
        .ok_or(BuildError::MissingFrom)?;
    let message_id = message_id(&from);

    let builder = Message::builder()
        .message_id(Some(message_id.clone()))
        .from(from)
        .subject(subject);
    let builder = with_mailboxes::<To>(builder, to);
    let builder = with_mailboxes::<Cc>(builder, cc);
    let builder = with_mailboxes::<Bcc>(builder, bcc);
    let builder = with_mailboxes::<ReplyTo>(builder, reply_to);
    let builder = with_custom_headers(builder, headers)?;

    let content = render::render(body, format, text_body)?;
    let message = assemble(builder, content, attachments)?;
    Ok(Mail {
        message_id,
        message,
    })
}

fn validate(request: &SendRequest) -> Result<(), BuildError> {
    if is_empty(&request.to) {
        return Err(BuildError::MissingRecipient);
    }
    if request.format == BodyFormat::Text && request.text_body.is_some() {
        return Err(BuildError::UnexpectedTextBody);
    }
    Ok(())
}

fn message_id(from: &Mailbox) -> String {
    let id = Uuid::new_v4().simple();
    let domain = from.email.domain();
    format!("<{id}@{domain}>")
}

fn is_empty(mailboxes: &Mailboxes) -> bool {
    mailboxes.iter().next().is_none()
}

fn with_mailboxes<H>(builder: MessageBuilder, mailboxes: Mailboxes) -> MessageBuilder
where
    H: Header + From<Mailboxes>,
{
    if is_empty(&mailboxes) {
        return builder;
    }
    builder.header(H::from(mailboxes))
}

fn with_custom_headers(
    builder: MessageBuilder,
    headers: BTreeMap<String, String>,
) -> Result<MessageBuilder, BuildError> {
    headers
        .into_iter()
        .try_fold(builder, |builder, (name, value)| {
            Ok(builder.raw_header(custom_header(name, value)?))
        })
}

fn custom_header(name: String, value: String) -> Result<HeaderValue, BuildError> {
    if RESERVED_HEADERS
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(&name))
    {
        return Err(BuildError::ReservedHeader(name));
    }
    if !is_valid_header_name(&name) {
        return Err(BuildError::InvalidHeaderName(name));
    }
    Ok(HeaderValue::new(HeaderName::new_from_ascii(name)?, value))
}

fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_HEADER_NAME_LEN
        && name
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b':')
}

fn is_valid_content_id(content_id: &str) -> bool {
    !content_id.is_empty()
        && content_id
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'<' && byte != b'>')
}

fn assemble(
    builder: MessageBuilder,
    content: Content,
    attachments: Vec<Attachment>,
) -> Result<Message, BuildError> {
    let (inline, attached): (Vec<_>, Vec<_>) = attachments
        .into_iter()
        .partition(|attachment| attachment.content_id.is_some());

    let body = content_part(content);
    let body = enclose(body, MultiPart::related(), attachment_parts(inline)?);
    let body = enclose(body, MultiPart::mixed(), attachment_parts(attached)?);

    let message = match body {
        Body::Single(part) => builder.singlepart(part),
        Body::Multi(part) => builder.multipart(part),
    }?;
    Ok(message)
}

fn content_part(content: Content) -> Body {
    match content {
        Content::Text(text) => Body::Single(SinglePart::plain(text)),
        Content::Html { html, text } => Body::Multi(MultiPart::alternative_plain_html(text, html)),
    }
}

fn enclose(body: Body, container: MultiPartBuilder, parts: Vec<SinglePart>) -> Body {
    if parts.is_empty() {
        return body;
    }
    let multipart = match body {
        Body::Single(part) => container.singlepart(part),
        Body::Multi(part) => container.multipart(part),
    };
    Body::Multi(parts.into_iter().fold(multipart, MultiPart::singlepart))
}

fn attachment_parts(attachments: Vec<Attachment>) -> Result<Vec<SinglePart>, BuildError> {
    attachments.into_iter().map(attachment_part).collect()
}

fn attachment_part(attachment: Attachment) -> Result<SinglePart, BuildError> {
    let content_type = attachment
        .content_type
        .unwrap_or_else(|| guess_content_type(&attachment.filename));
    let part = match attachment.content_id {
        Some(content_id) if is_valid_content_id(&content_id) => {
            AttachmentPart::new_inline_with_name(content_id, attachment.filename)
        }
        Some(content_id) => return Err(BuildError::InvalidContentId(content_id)),
        None => AttachmentPart::new(attachment.filename),
    };
    Ok(part.body(attachment.content, content_type))
}

fn guess_content_type(filename: &str) -> ContentType {
    ContentType::from(mime_guess::from_path(filename).first_or_octet_stream())
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use serde::Deserialize;
    use serde_json::{Value, json};

    use super::*;

    type TestResult = Result<(), Box<dyn Error>>;

    fn request(value: &Value) -> Result<SendRequest, serde_json::Error> {
        SendRequest::deserialize(value)
    }

    fn formatted(mail: &Mail) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(mail.message.formatted())
    }

    #[test]
    fn builds_markdown_mail() -> TestResult {
        let request = request(&json!({
            "from": "Foo <foo@domain.tld>",
            "to": ["bar@domain.tld", "baz@domain.tld"],
            "cc": "qux@domain.tld",
            "bcc": "quux@domain.tld",
            "reply_to": "corge@domain.tld",
            "subject": "Hello",
            "body": "# Hi\n\nThis is **markdown**.",
            "headers": { "X-Campaign": "test", "In-Reply-To": "<abc@domain.tld>" },
        }))?;
        let mail = build(request, None)?;
        let raw = formatted(&mail)?;

        assert!(mail.message_id.starts_with('<'));
        assert!(mail.message_id.ends_with("@domain.tld>"));
        assert!(raw.contains(&format!("Message-ID: {}", mail.message_id)));
        assert!(raw.contains("From: Foo <foo@domain.tld>"));
        assert!(raw.contains("To: bar@domain.tld, baz@domain.tld"));
        assert!(raw.contains("Cc: qux@domain.tld"));
        assert!(raw.contains("Reply-To: corge@domain.tld"));
        assert!(raw.contains("Subject: Hello"));
        assert!(raw.contains("X-Campaign: test"));
        assert!(raw.contains("In-Reply-To: <abc@domain.tld>"));
        assert!(raw.contains("Content-Type: multipart/alternative"));
        assert!(raw.contains("Content-Type: text/plain; charset=utf-8"));
        assert!(raw.contains("Content-Type: text/html; charset=utf-8"));
        assert!(!raw.contains("quux@domain.tld"));

        let recipients: Vec<String> = mail
            .message
            .envelope()
            .to()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            recipients,
            [
                "bar@domain.tld",
                "baz@domain.tld",
                "qux@domain.tld",
                "quux@domain.tld"
            ]
        );
        Ok(())
    }

    #[test]
    fn builds_plain_text_mail() -> TestResult {
        let request = request(&json!({
            "from": "foo@domain.tld",
            "to": "bar@domain.tld",
            "subject": "Hello",
            "body": "Just text",
            "format": "text",
        }))?;
        let raw = formatted(&build(request, None)?)?;
        assert!(raw.contains("Content-Type: text/plain; charset=utf-8"));
        assert!(!raw.contains("multipart"));
        assert!(!raw.contains("text/html"));
        Ok(())
    }

    #[test]
    fn falls_back_to_default_from() -> TestResult {
        let default_from: Mailbox = "Foo <foo@mail.domain.tld>".parse()?;
        let request = request(&json!({ "to": "bar@domain.tld", "subject": "S", "body": "B" }))?;
        let mail = build(request, Some(&default_from))?;
        assert!(formatted(&mail)?.contains("From: Foo <foo@mail.domain.tld>"));
        assert!(mail.message_id.ends_with("@mail.domain.tld>"));
        Ok(())
    }

    #[test]
    fn nests_attachments() -> TestResult {
        let request = request(&json!({
            "from": "foo@domain.tld",
            "to": "bar@domain.tld",
            "subject": "Report",
            "body": "<p><img src=\"cid:logo\"></p>",
            "format": "html",
            "attachments": [
                { "filename": "logo.png", "content": "iVBORw0KGgo=", "content_id": "logo" },
                { "filename": "report.pdf", "content": "JVBERi0=" },
                { "filename": "data.bin", "content": "AAE=", "content_type": "text/csv" },
            ],
        }))?;
        let raw = formatted(&build(request, None)?)?;

        let position = |needle: &str| raw.find(needle).ok_or(format!("missing {needle}"));
        let mixed = position("Content-Type: multipart/mixed")?;
        let related = position("Content-Type: multipart/related")?;
        let alternative = position("Content-Type: multipart/alternative")?;
        let logo = position("Content-ID: <logo>")?;
        let report = position("Content-Disposition: attachment; filename=\"report.pdf\"")?;
        assert!(mixed < related && related < alternative && alternative < logo && logo < report);
        assert!(raw.contains("Content-Type: image/png"));
        assert!(raw.contains("Content-Type: application/pdf"));
        assert!(raw.contains("Content-Type: text/csv"));
        Ok(())
    }

    #[test]
    fn rejects_invalid_requests() -> TestResult {
        let cases = [
            (json!({ "to": [] }), "at least one recipient"),
            (
                json!({ "format": "text", "text_body": "plain" }),
                "text_body",
            ),
            (
                json!({ "headers": { "content-type": "text/plain" } }),
                "managed by the proxy",
            ),
            (
                json!({ "headers": { "X-Evil\r\nBcc": "baz@domain.tld" } }),
                "is invalid",
            ),
            (json!({ "headers": { "Bad Name": "value" } }), "is invalid"),
            (
                json!({ "attachments": [{ "filename": "a.png", "content": "", "content_id": "<a>" }] }),
                "content_id",
            ),
        ];
        for (patch, expected) in cases {
            let mut case = json!({
                "from": "foo@domain.tld",
                "to": "bar@domain.tld",
                "subject": "S",
                "body": "B",
            });
            if let (Value::Object(case), Value::Object(patch)) = (&mut case, patch) {
                case.extend(patch);
            }
            let error = build(request(&case)?, None)
                .err()
                .map(|error| error.to_string());
            assert!(
                error
                    .as_deref()
                    .is_some_and(|error| error.contains(expected)),
                "{case} should be rejected with {expected:?}, got {error:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn requires_from() -> TestResult {
        let request = request(&json!({ "to": "bar@domain.tld", "subject": "S", "body": "B" }))?;
        assert!(matches!(build(request, None), Err(BuildError::MissingFrom)));
        Ok(())
    }

    #[test]
    fn encodes_header_values() -> TestResult {
        let request = request(&json!({
            "from": "foo@domain.tld",
            "to": "bar@domain.tld",
            "subject": "Hi\r\nBcc: baz@domain.tld",
            "body": "B",
            "headers": { "X-Note": "line\r\nBcc: baz@domain.tld" },
        }))?;
        let raw = formatted(&build(request, None)?)?;
        assert!(!raw.contains("\r\nBcc:"));
        Ok(())
    }
}
