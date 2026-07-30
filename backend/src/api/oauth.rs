use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Redirect,
    routing::get,
};
use serde::Deserialize;

use crate::{AppState, oauth::Provider};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/oauth/{provider}/authorize", get(authorize))
        .route("/oauth/{provider}/callback", get(callback))
}

async fn authorize(
    Path(provider_str): Path<String>,
    State(state): State<AppState>,
) -> Result<Redirect, (StatusCode, String)> {
    let provider = Provider::from_str(&provider_str).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("Unknown provider: {provider_str}"),
        )
    })?;

    let redirect_uri = callback_url(&provider_str);
    let cfg = state
        .config
        .read()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?;

    let (auth_endpoint, scopes, creds) = match provider {
        Provider::Forgejo => (
            format!(
                "{}/login/oauth/authorize",
                cfg.forgejo.url.trim_end_matches('/')
            ),
            vec!["read:repository"],
            cfg.forgejo.oauth.clone(),
        ),
        Provider::Google => (
            "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            vec![
                "https://www.googleapis.com/auth/drive.file",
                "https://www.googleapis.com/auth/gmail.send",
            ],
            cfg.google.oauth.clone(),
        ),
    };
    drop(cfg);

    // Google requires access_type=offline to issue a refresh token, and
    // prompt=consent to re-issue one on subsequent authorizations.
    let extra: &[(&str, &str)] = match provider {
        Provider::Google => &[("access_type", "offline"), ("prompt", "consent")],
        _ => &[],
    };

    let (url, _csrf) = state.oauth.authorization_url(
        provider,
        &creds,
        &auth_endpoint,
        &redirect_uri,
        &scopes,
        extra,
    );

    Ok(Redirect::to(&url))
}

#[derive(Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn callback(
    Path(provider_str): Path<String>,
    Query(params): Query<CallbackParams>,
    State(state): State<AppState>,
) -> Result<Redirect, (StatusCode, String)> {
    let provider = Provider::from_str(&provider_str).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("Unknown provider: {provider_str}"),
        )
    })?;

    if let Some(err) = params.error {
        tracing::warn!("OAuth error from {provider_str}: {err}");
        return Ok(Redirect::to(&format!(
            "/config?oauth_error={}",
            percent_encoding::utf8_percent_encode(&err, percent_encoding::NON_ALPHANUMERIC)
        )));
    }

    let code = params
        .code
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing code".into()))?;
    let csrf_state = params
        .state
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing state".into()))?;

    let redirect_uri = callback_url(&provider_str);
    let (token_endpoint, creds) = {
        let cfg = state
            .config
            .read()
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?;
        match provider {
            Provider::Forgejo => (
                format!(
                    "{}/login/oauth/access_token",
                    cfg.forgejo.url.trim_end_matches('/')
                ),
                cfg.forgejo.oauth.clone(),
            ),
            Provider::Google => (
                "https://oauth2.googleapis.com/token".to_string(),
                cfg.google.oauth.clone(),
            ),
        }
        // cfg (RwLockReadGuard) is dropped here
    };

    state
        .oauth
        .exchange_code(&csrf_state, &code, &creds, &token_endpoint, &redirect_uri)
        .await
        .map_err(|e| {
            tracing::error!("Code exchange failed for {provider_str}: {e}");
            (StatusCode::BAD_GATEWAY, e)
        })?;

    tracing::info!("OAuth2 connected: {provider_str}");
    Ok(Redirect::to("/config?oauth_connected=1"))
}

/// Derive the OAuth2 callback URL from the `BASE_URL` env var.
/// This **must** match the redirect URI registered in the OAuth app on the
/// provider (Forgejo settings / Google Cloud Console).
/// Defaults to `http://localhost:3000` for local development.
fn callback_url(provider: &str) -> String {
    let base = std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
    format!("{base}/api/oauth/{provider}/callback")
}
