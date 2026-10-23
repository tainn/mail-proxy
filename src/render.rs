use pulldown_cmark::{Event, Options, Parser, html};

use crate::api::BodyFormat;

const PLAIN_TEXT_WIDTH: usize = 80;

const MARKDOWN_OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_FOOTNOTES);

const DOCUMENT_HEAD: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
body { margin: 0; padding: 0; background-color: #ffffff; }
.content { max-width: 680px; margin: 0 auto; padding: 24px 16px; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; font-size: 16px; line-height: 1.6; color: #1f2328; word-wrap: break-word; }
h1, h2, h3, h4, h5, h6 { margin: 24px 0 16px; font-weight: 600; line-height: 1.25; }
h1 { font-size: 1.75em; }
h2 { font-size: 1.4em; padding-bottom: 0.3em; border-bottom: 1px solid #d1d9e0; }
h3 { font-size: 1.2em; }
h4, h5, h6 { font-size: 1em; }
p, ul, ol, dl, blockquote, pre, table { margin: 0 0 16px; }
ul, ol { padding-left: 2em; }
li + li { margin-top: 0.25em; }
a { color: #0969da; text-decoration: underline; }
code { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace; font-size: 85%; padding: 0.2em 0.4em; background-color: #eff1f3; border-radius: 6px; }
pre { padding: 16px; overflow: auto; line-height: 1.45; background-color: #f6f8fa; border-radius: 6px; }
pre code { padding: 0; font-size: 85%; background-color: transparent; border-radius: 0; }
blockquote { margin-left: 0; margin-right: 0; padding: 0 1em; color: #59636e; border-left: 4px solid #d1d9e0; }
table { border-collapse: collapse; border-spacing: 0; }
th, td { padding: 6px 13px; border: 1px solid #d1d9e0; }
th { font-weight: 600; background-color: #f6f8fa; }
img { max-width: 100%; height: auto; border: 0; }
hr { height: 0; margin: 24px 0; border: 0; border-top: 1px solid #d1d9e0; }
</style>
</head>
<body>
<div class="content">
"#;

const DOCUMENT_TAIL: &str = "</div>\n</body>\n</html>\n";

pub enum Content {
    Text(String),
    Html { html: String, text: String },
}

pub fn render(
    body: String,
    format: BodyFormat,
    text: Option<String>,
) -> Result<Content, html2text::Error> {
    match format {
        BodyFormat::Text => Ok(Content::Text(body)),
        BodyFormat::Html => html_content(body, text),
        BodyFormat::Markdown => markdown_content(&body, text),
    }
}

fn html_content(html: String, text: Option<String>) -> Result<Content, html2text::Error> {
    let text = plain_text(text, &html)?;
    Ok(Content::Html { html, text })
}

fn markdown_content(markdown: &str, text: Option<String>) -> Result<Content, html2text::Error> {
    let fragment = markdown_to_html(markdown);
    let text = plain_text(text, &fragment)?;
    let html = [DOCUMENT_HEAD, &fragment, DOCUMENT_TAIL].concat();
    Ok(Content::Html { html, text })
}

fn plain_text(text: Option<String>, html: &str) -> Result<String, html2text::Error> {
    match text {
        Some(text) => Ok(text),
        None => html_to_text(html),
    }
}

fn markdown_to_html(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, MARKDOWN_OPTIONS).map(task_marker_as_text);
    let mut output = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut output, parser);
    output
}

fn task_marker_as_text(event: Event<'_>) -> Event<'_> {
    match event {
        Event::TaskListMarker(true) => Event::Text("☑ ".into()),
        Event::TaskListMarker(false) => Event::Text("☐ ".into()),
        other => other,
    }
}

fn html_to_text(html: &str) -> Result<String, html2text::Error> {
    html2text::config::plain()
        .no_link_wrapping()
        .string_from_read(html.as_bytes(), PLAIN_TEXT_WIDTH)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    fn html_and_text(content: Content) -> Option<(String, String)> {
        match content {
            Content::Html { html, text } => Some((html, text)),
            Content::Text(_) => None,
        }
    }

    #[test]
    fn renders_markdown_into_styled_document() -> Result<(), Box<dyn Error>> {
        let markdown = "# Title\n\nSome **bold** and ~~old~~ [link](https://domain.tld).\n\n\
            | a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n<p class=\"raw\">raw</p>\n";
        let content = render(markdown.to_owned(), BodyFormat::Markdown, None)?;
        let (html, text) = html_and_text(content).ok_or("expected html content")?;
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<del>old</del>"));
        assert!(html.contains("<table>"));
        assert!(html.contains("☑ done"));
        assert!(html.contains("☐ todo"));
        assert!(!html.contains("<input"));
        assert!(html.contains("<p class=\"raw\">raw</p>"));
        assert!(html.ends_with(DOCUMENT_TAIL));
        assert!(text.contains("Title"));
        assert!(text.contains("https://domain.tld"));
        assert!(!text.contains('<'));
        Ok(())
    }

    #[test]
    fn keeps_html_as_is() -> Result<(), Box<dyn Error>> {
        let body = "<html><head><style>p { color: red; }</style></head>\
            <body><p>Hello <b>world</b></p></body></html>";
        let content = render(body.to_owned(), BodyFormat::Html, None)?;
        let (html, text) = html_and_text(content).ok_or("expected html content")?;
        assert_eq!(html, body);
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert!(!text.contains("color"));
        Ok(())
    }

    #[test]
    fn prefers_explicit_text() -> Result<(), Box<dyn Error>> {
        let content = render(
            "<p>Hi</p>".to_owned(),
            BodyFormat::Html,
            Some("Hey".to_owned()),
        )?;
        let (_, text) = html_and_text(content).ok_or("expected html content")?;
        assert_eq!(text, "Hey");
        Ok(())
    }

    #[test]
    fn keeps_plain_text() -> Result<(), Box<dyn Error>> {
        let content = render("**not bold**".to_owned(), BodyFormat::Text, None)?;
        assert!(matches!(content, Content::Text(text) if text == "**not bold**"));
        Ok(())
    }
}
