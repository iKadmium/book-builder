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

// ── Open WebUI ────────────────────────────────────────────────────────────────

/// Upload the markdown file to Open WebUI, create a new chat with it attached
/// and a critique prompt (unsent), and return the URL to open in the browser.
pub async fn deploy_to_openwebui(
    endpoint: &str,
    api_key: &str,
    md_path: &Path,
    title: &str,
) -> Result<String, String> {
    if endpoint.is_empty() {
        return Err("OPEN_WEBUI_ENDPOINT is not configured".into());
    }
    if api_key.is_empty() {
        return Err("OPEN_WEBUI_API_KEY is not configured".into());
    }

    let base = endpoint.trim_end_matches('/');
    let md_bytes = tokio::fs::read(md_path)
        .await
        .map_err(|e| format!("Failed to read markdown file: {e}"))?;

    let filename = md_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("book.md")
        .to_string();

    let client = reqwest::Client::new();

    // 1. Upload the file.
    let file_part = reqwest::multipart::Part::bytes(md_bytes)
        .file_name(filename.clone())
        .mime_str("text/markdown")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new().part("file", file_part);

    let upload_resp = client
        .post(format!("{base}/api/v1/files/"))
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("File upload failed: {e}"))?;

    if !upload_resp.status().is_success() {
        let status = upload_resp.status();
        let body = upload_resp.text().await.unwrap_or_default();
        return Err(format!("File upload error {status}: {body}"));
    }

    let upload_result: serde_json::Value = upload_resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse upload response: {e}"))?;

    let file_id = upload_result["id"]
        .as_str()
        .ok_or_else(|| "No file id in upload response".to_string())?
        .to_string();

    // 2. Create a new chat with the file attached and a critique prompt.
    let msg_id = new_uuid();
    let now_ts = chrono::Utc::now().timestamp();
    let prompt = format!(
        "I've attached the manuscript for \"{title}\". \
        Please provide a detailed critique covering: plot and pacing, \
        character development, dialogue and prose style, \
        consistency and world-building, \
        and overall strengths and areas for improvement."
    );

    let message = serde_json::json!({
        "id": msg_id,
        "parentId": null,
        "childrenIds": [],
        "role": "user",
        "content": prompt,
        "files": [{
            "type": "file",
            "id": file_id,
            "name": filename,
            "url": format!("/api/v1/files/{file_id}/content"),
        }],
        "timestamp": now_ts,
    });

    let chat_body = serde_json::json!({
        "chat": {
            "title": format!("{title} \u{2014} Critique Request"),
            "models": [],
            "messages": [message],
            "history": {
                "messages": { &msg_id: &message },
                "currentId": msg_id,
            },
        }
    });

    let create_resp = client
        .post(format!("{base}/api/v1/chats/new"))
        .bearer_auth(api_key)
        .json(&chat_body)
        .send()
        .await
        .map_err(|e| format!("Chat creation failed: {e}"))?;

    if !create_resp.status().is_success() {
        let status = create_resp.status();
        let body = create_resp.text().await.unwrap_or_default();
        return Err(format!("Chat creation error {status}: {body}"));
    }

    let create_result: serde_json::Value = create_resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse chat creation response: {e}"))?;

    let chat_id = create_result["id"]
        .as_str()
        .ok_or_else(|| "No chat id in response".to_string())?;

    Ok(format!("{base}/c/{chat_id}"))
}

fn new_uuid() -> String {
    use rand::Rng as _;
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}
