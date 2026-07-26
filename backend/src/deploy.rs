//! Email delivery via the Gmail API (OAuth2 bearer token).

use std::path::Path;

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};

/// Send `epub_path` to `to` via the Gmail API.
/// `access_token` must be a valid Google OAuth2 token with the
/// `https://www.googleapis.com/auth/gmail.send` scope.
pub async fn deploy_book(
    from: &str,
    to: &str,
    access_token: &str,
    epub_path: &Path,
    title: &str,
) -> Result<(), String> {
    if to.is_empty() {
        return Err("Recipient address (to) is not configured".into());
    }

    let epub_bytes = tokio::fs::read(epub_path)
        .await
        .map_err(|e| format!("Failed to read EPUB at {}: {e}", epub_path.display()))?;

    let filename = epub_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("book.epub")
        .to_string();

    let raw = build_raw_message(from, to, title, &epub_bytes, &filename);

    let client = reqwest::Client::new();
    let resp = client
        .post("https://gmail.googleapis.com/gmail/v1/users/me/messages/send")
        .bearer_auth(access_token)
        .json(&serde_json::json!({ "raw": raw }))
        .send()
        .await
        .map_err(|e| format!("Gmail API request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail API error {status}: {body}"));
    }

    tracing::info!("Deployed '{title}' to {to}");
    Ok(())
}

/// Build a base64url-encoded RFC 2822 MIME message for the Gmail API `raw` field.
fn build_raw_message(
    from: &str,
    to: &str,
    subject: &str,
    epub_bytes: &[u8],
    filename: &str,
) -> String {
    let boundary = "----=_Part_BookBuilder_0";

    // Standard base64, split into 76-char lines per MIME spec.
    let epub_b64 = STANDARD.encode(epub_bytes);
    let epub_b64_wrapped = epub_b64
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join("\r\n");

    let message = format!(
        "From: {from}\r\n\
         To: {to}\r\n\
         Subject: {subject}\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\
         \r\n\
         --{boundary}\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         Please find '{filename}' attached.\r\n\
         \r\n\
         --{boundary}\r\n\
         Content-Type: application/epub+zip; name=\"{filename}\"\r\n\
         Content-Disposition: attachment; filename=\"{filename}\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         {epub_b64_wrapped}\r\n\
         --{boundary}--"
    );

    URL_SAFE_NO_PAD.encode(message.as_bytes())
}
