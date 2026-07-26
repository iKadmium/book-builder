use std::path::Path;

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    message::{Attachment, MultiPart, SinglePart, header::ContentType},
    transport::smtp::authentication::Credentials,
    Message,
};

use crate::config::EmailConfig;

pub async fn deploy_book(config: &EmailConfig, epub_path: &Path, title: &str) -> Result<(), String> {
    if config.smtp_host.is_empty() {
        return Err("Email is not configured (smtp_host is empty)".into());
    }

    let epub_bytes = tokio::fs::read(epub_path)
        .await
        .map_err(|e| format!("Failed to read EPUB at {}: {e}", epub_path.display()))?;

    let filename = epub_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("book.epub")
        .to_string();

    let content_type: ContentType = "application/epub+zip"
        .parse()
        .map_err(|e| format!("Invalid content type: {e}"))?;

    let email = Message::builder()
        .from(
            config
                .from
                .parse()
                .map_err(|e| format!("Invalid from address '{}': {e}", config.from))?,
        )
        .to(config
            .to
            .parse()
            .map_err(|e| format!("Invalid to address '{}': {e}", config.to))?)
        .subject(title)
        .multipart(
            MultiPart::mixed()
                .singlepart(SinglePart::plain(format!(
                    "Please find '{filename}' attached."
                )))
                .singlepart(Attachment::new(filename).body(epub_bytes, content_type)),
        )
        .map_err(|e| format!("Failed to build email: {e}"))?;

    let creds = Credentials::new(
        config.smtp_username.clone(),
        config.smtp_password.clone(),
    );

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp_host)
        .map_err(|e| format!("Failed to create SMTP transport: {e}"))?
        .port(config.smtp_port)
        .credentials(creds)
        .build();

    mailer
        .send(email)
        .await
        .map_err(|e| format!("Failed to send email: {e}"))?;

    tracing::info!("Deployed '{title}' to {}", config.to);
    Ok(())
}
