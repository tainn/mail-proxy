# mail-proxy

Proxy API for mails.

Accepts a JSON request over HTTP and relays it as a properly structured MIME mail over SMTP.

- Bodies can be written in **Markdown** (default), **HTML** or plain text.
- Markdown is rendered into a styled, mail-client friendly HTML document.
- HTML and Markdown mails always include a plain-text alternative, derived automatically unless provided.
- Supports cc, bcc, reply-to, custom headers, file attachments and inline images (`cid:`).

## Configuration

All configuration is done via environment variables. Empty values are treated as unset.

| Variable            | Required | Default        | Description                                                                                         |
|---------------------|----------|----------------|-----------------------------------------------------------------------------------------------------|
| `SMTP_DOMAIN`       | yes      |                | SMTP server host name.                                                                              |
| `SMTP_PORT`         | yes      |                | SMTP server port.                                                                                   |
| `SMTP_TLS`          | no       | by port        | `starttls`, `opportunistic`, `tls` (implicit) or `none`. Defaults to `tls` on 465, else `starttls`. |
| `SMTP_USERNAME`     | no       |                | SMTP username. Must be set together with `SMTP_PASSWORD`.                                           |
| `SMTP_PASSWORD`     | no       |                | SMTP password. Must be set together with `SMTP_USERNAME`.                                           |
| `SMTP_HELO_NAME`    | no       | `[127.0.0.1]`  | Name sent with `EHLO`. Set it to a FQDN if your server checks it.                                   |
| `SMTP_TIMEOUT_SECS` | no       | `30`           | SMTP connection and command timeout.                                                                |
| `MAIL_DEFAULT_FROM` | no       |                | Sender used when a request has no `from`, e.g. `Foo <foo@domain.tld>`.                              |
| `API_TOKEN`         | no       |                | Bearer token required on `POST /send`. Authentication is disabled when unset.                       |
| `LISTEN_ADDR`       | no       | `0.0.0.0:3000` | HTTP listen address.                                                                                |
| `MAX_REQUEST_BYTES` | no       | `26214400`     | Maximum request body size (25 MiB), attachments included.                                           |
| `RUST_LOG`          | no       | `info`         | Log filter.                                                                                         |

TLS certificates are verified against the system trust store.

## API

### `POST /send`

Requires `Content-Type: application/json` and, if `API_TOKEN` is set, `Authorization: Bearer <token>`.

| Field         | Type                 | Required | Description                                                                                  |
|---------------|----------------------|----------|----------------------------------------------------------------------------------------------|
| `to`          | mailboxes            | yes      | At least one recipient.                                                                      |
| `subject`     | string               | yes      | Subject line, any unicode.                                                                   |
| `body`        | string               | yes      | Mail content in the given `format`.                                                          |
| `from`        | mailbox              | no*      | Sender. *Required if `MAIL_DEFAULT_FROM` is not configured.                                  |
| `cc`          | mailboxes            | no       | Carbon copy recipients.                                                                      |
| `bcc`         | mailboxes            | no       | Blind carbon copy recipients, never visible in the delivered headers.                        |
| `reply_to`    | mailboxes            | no       | Reply-To addresses.                                                                          |
| `format`      | string               | no       | `markdown` (default), `html` or `text`.                                                      |
| `text_body`   | string               | no       | Plain-text alternative for `markdown` and `html`. Derived from the body when omitted.        |
| `attachments` | array of attachments | no       | Files to attach.                                                                             |
| `headers`     | object               | no       | Extra headers, e.g. `{"In-Reply-To": "<id@domain.tld>", "List-Unsubscribe": "<https://…>"}`. |

A **mailbox** is either a string (`"foo@domain.tld"`, `"Foo <foo@domain.tld>"`) or an object
(`{"name": "Foo", "email": "foo@domain.tld"}`). **Mailboxes** are either an array of mailboxes or a single comma
separated string.

An **attachment** is an object:

| Field          | Type   | Required | Description                                                                             |
|----------------|--------|----------|-----------------------------------------------------------------------------------------|
| `filename`     | string | yes      | File name shown to the recipient.                                                       |
| `content`      | string | yes      | Standard base64 file content. Whitespace and missing padding are tolerated.             |
| `content_type` | string | no       | MIME type, guessed from `filename` when omitted.                                        |
| `content_id`   | string | no       | Makes the attachment inline, so the body can reference it, e.g. `<img src="cid:logo">`. |

Headers managed by the proxy (`From`, `To`, `Cc`, `Bcc`, `Reply-To`, `Sender`, `Subject`, `Date`, `Message-ID`,
`MIME-Version` and `Content-*` structure headers) cannot be set via `headers`.

#### Formats

- `markdown`: CommonMark with tables, strikethrough, task lists and footnotes. Inline HTML is passed through. The result
  is wrapped in a styled HTML document.
- `html`: sent exactly as provided, so you are in full control of the layout and styling.
- `text`: sent as a single `text/plain` part.

#### Response

`200 OK`

```json
{
  "message_id": "<0f9d2c1e5b6a4c7e8f1a2b3c4d5e6f70@domain.tld>"
}
```

Errors are returned as `{"error": "..."}`:

| Status | Reason                                                      |
|--------|-------------------------------------------------------------|
| `400`  | Malformed JSON.                                             |
| `401`  | Missing or invalid bearer token.                            |
| `413`  | Request body larger than `MAX_REQUEST_BYTES`.               |
| `415`  | Missing `Content-Type: application/json`.                   |
| `422`  | Invalid field value, e.g. a bad address or reserved header. |
| `502`  | SMTP server rejected the mail or could not be reached.      |
| `504`  | SMTP server timed out.                                      |

#### Examples

```sh
curl -sS -X POST http://localhost:3000/send \
  -H "Content-Type: application/json" \
  --data-binary @- <<'EOF'
{
  "to": "foo@domain.tld",
  "subject": "mail-proxy: markdown test",
  "body": "# Hello from mail-proxy\n\nThis mail was written in **Markdown** and rendered to *HTML*.\n\n## Status\n\n| Service | State | Uptime |\n|---|---|---:|\n| api | up | 99.98% |\n| worker | degraded | 97.10% |\n\n## Todo\n\n- [x] Send a markdown mail\n- [ ] Send an html mail\n\n> Quotes, `inline code` and [links](https://www.rust-lang.org) work too.\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n"
}
EOF
```

```sh
curl -sS -X POST http://localhost:3000/send \
  -H "Content-Type: application/json" \
  --data-binary @- <<'EOF'
{
  "from": "Foo <foo@domain.tld>",
  "to": ["Bar <bar@domain.tld>", "baz@domain.tld"],
  "subject": "mail-proxy: html test",
  "format": "html",
  "body": "<!DOCTYPE html><html><body style=\"margin:0;padding:24px;background:#f6f8fa;font-family:Arial,sans-serif;\"><table role=\"presentation\" width=\"100%\" style=\"max-width:560px;margin:0 auto;background:#ffffff;border-radius:8px;\"><tr><td style=\"padding:32px;\"><h1 style=\"margin:0 0 16px;font-size:24px;color:#1f2328;\">Hello from mail-proxy</h1><p style=\"margin:0 0 24px;font-size:16px;line-height:1.5;color:#424a53;\">This mail was written in <strong>HTML</strong> with inline styles.</p><a href=\"https://www.rust-lang.org\" style=\"display:inline-block;padding:12px 20px;background:#0969da;color:#ffffff;text-decoration:none;border-radius:6px;font-weight:bold;\">Visit rust-lang.org</a></td></tr></table></body></html>"
}
EOF
```

### `GET /health`

Returns `200 OK` with body `ok`.
