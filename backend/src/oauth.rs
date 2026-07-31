//! Provider-agnostic OAuth2 authorization-code + PKCE flow.
//!
//! Tokens are persisted to a JSON file so they survive process restarts.
//! The pending in-flight PKCE state is kept in memory only (short-lived).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::config::OAuth2Credentials;

// ── Provider ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Forgejo,
    Google,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Forgejo => "forgejo",
            Self::Google => "google",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "forgejo" => Some(Self::Forgejo),
            "google" => Some(Self::Google),
            _ => None,
        }
    }
}

// ── Token types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl TokenSet {
    fn is_valid(&self) -> bool {
        // Treat a token with no expiry info as valid.
        self.expires_at
            .map(|exp| Utc::now() < exp - Duration::seconds(60))
            .unwrap_or(true)
    }
}

/// Schema for `tokens.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TokenFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    forgejo: Option<TokenSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    google: Option<TokenSet>,
}

impl TokenFile {
    fn get(&self, provider: Provider) -> Option<&TokenSet> {
        match provider {
            Provider::Forgejo => self.forgejo.as_ref(),
            Provider::Google => self.google.as_ref(),
        }
    }

    fn set(&mut self, provider: Provider, tokens: TokenSet) {
        match provider {
            Provider::Forgejo => self.forgejo = Some(tokens),
            Provider::Google => self.google = Some(tokens),
        }
    }
}

// ── Internal ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Deserialize)]
struct OAuthErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

struct PendingAuth {
    provider: Provider,
    pkce_verifier: String,
    created_at: std::time::Instant,
}

// ── OAuthManager ─────────────────────────────────────────────────────────────

/// Thread-safe OAuth2 manager with file-backed token storage.
///
/// Tokens are written to `tokens_path` after every successful exchange or
/// refresh, so they survive process restarts.  The in-flight PKCE/CSRF state
/// is kept in memory only (it is short-lived and tied to a browser session).
pub struct OAuthManager {
    tokens: RwLock<TokenFile>,
    tokens_path: PathBuf,
    /// In-flight authorization requests: csrf_state → pending PKCE data.
    pending: Mutex<HashMap<String, PendingAuth>>,
    http: reqwest::Client,
    encryption_key: [u8; 32],
}

impl OAuthManager {
    /// Create a manager, loading any previously saved tokens from `tokens_path`.
    /// `encryption_key` is a 32-byte AES-256-GCM key; derive one with [`derive_key`].
    pub fn load(tokens_path: PathBuf, encryption_key: [u8; 32]) -> Arc<Self> {
        let tokens = load_token_file(&tokens_path, &encryption_key);
        Arc::new(Self {
            tokens: RwLock::new(tokens),
            tokens_path,
            pending: Mutex::new(HashMap::new()),
            http: reqwest::Client::new(),
            encryption_key,
        })
    }

    /// Build the provider authorization redirect URL and record the in-flight
    /// PKCE state.  Returns `(redirect_url, csrf_state)`.
    ///
    /// `extra_params` is appended verbatim as additional percent-encoded query
    /// parameters, e.g. `&[("access_type", "offline"), ("prompt", "consent")]`
    /// for Google to request a refresh token.
    pub fn authorization_url(
        &self,
        provider: Provider,
        creds: &OAuth2Credentials,
        auth_endpoint: &str,
        redirect_uri: &str,
        scopes: &[&str],
        extra_params: &[(&str, &str)],
    ) -> (String, String) {
        let state = random_token();
        let (verifier, challenge) = pkce_pair();

        {
            let mut pending = self.pending.lock().unwrap();
            // Prune stale entries (older than 10 minutes) to avoid unbounded growth.
            let now = std::time::Instant::now();
            pending.retain(|_, v| {
                now.duration_since(v.created_at) < std::time::Duration::from_secs(600)
            });
            pending.insert(
                state.clone(),
                PendingAuth {
                    provider,
                    pkce_verifier: verifier,
                    created_at: now,
                },
            );
        }

        let mut url = reqwest::Url::parse(auth_endpoint)
            .unwrap_or_else(|e| panic!("Invalid auth endpoint URL '{auth_endpoint}': {e}"));
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &creds.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", &scopes.join(" "))
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        for (k, v) in extra_params {
            url.query_pairs_mut().append_pair(k, v);
        }

        (url.into(), state)
    }

    /// Exchange an authorization code (from the provider callback) for tokens.
    /// Validates the CSRF `state` and PKCE verifier, then persists the result.
    /// Returns the provider on success.
    pub async fn exchange_code(
        &self,
        csrf_state: &str,
        code: &str,
        creds: &OAuth2Credentials,
        token_endpoint: &str,
        redirect_uri: &str,
    ) -> Result<Provider, String> {
        let (provider, verifier) = {
            let mut pending = self.pending.lock().unwrap();
            let p = pending
                .remove(csrf_state)
                .ok_or_else(|| "Unknown or expired OAuth state".to_string())?;
            (p.provider, p.pkce_verifier)
        };

        let http_resp = self
            .http
            .post(token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("client_id", creds.client_id.as_str()),
                ("client_secret", creds.client_secret.as_str()),
                ("code_verifier", verifier.as_str()),
            ])
            .send()
            .await
            .map_err(|e| format!("Token request failed: {e}"))?;

        let resp = parse_token_response(http_resp).await?;
        self.persist(provider, resp);
        Ok(provider)
    }

    /// Refresh the stored access token for `provider`.
    pub async fn refresh(
        &self,
        provider: Provider,
        creds: &OAuth2Credentials,
        token_endpoint: &str,
    ) -> Result<String, String> {
        let existing_refresh = self
            .tokens
            .read()
            .unwrap()
            .get(provider)
            .and_then(|t| t.refresh_token.clone())
            .ok_or_else(|| "No refresh token stored for this provider".to_string())?;

        let http_resp = self
            .http
            .post(token_endpoint)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", existing_refresh.as_str()),
                ("client_id", creds.client_id.as_str()),
                ("client_secret", creds.client_secret.as_str()),
            ])
            .send()
            .await
            .map_err(|e| format!("Refresh request failed: {e}"))?;

        let resp = parse_token_response(http_resp).await?;

        let access_token = resp.access_token.clone();
        // Keep the old refresh token if the server didn't issue a new one.
        let resp = TokenResponse {
            refresh_token: resp.refresh_token.or(Some(existing_refresh)),
            ..resp
        };
        self.persist(provider, resp);
        Ok(access_token)
    }

    /// Return a valid access token for `provider`, auto-refreshing if expired.
    /// Returns `None` if the user has not yet authorized this provider.
    pub async fn token(
        &self,
        provider: Provider,
        creds: &OAuth2Credentials,
        token_endpoint: &str,
    ) -> Option<String> {
        {
            let tokens = self.tokens.read().unwrap();
            if let Some(t) = tokens.get(provider)
                && t.is_valid()
            {
                return Some(t.access_token.clone());
            }
        }
        self.refresh(provider, creds, token_endpoint).await.ok()
    }

    /// Whether we have any tokens on file for this provider.
    pub fn is_connected(&self, provider: Provider) -> bool {
        self.tokens.read().unwrap().get(provider).is_some()
    }

    fn persist(&self, provider: Provider, resp: TokenResponse) {
        let expires_at = resp
            .expires_in
            .map(|secs| Utc::now() + Duration::seconds(secs));
        let token_set = TokenSet {
            access_token: resp.access_token,
            refresh_token: resp.refresh_token,
            expires_at,
        };
        let mut tokens = self.tokens.write().unwrap();
        tokens.set(provider, token_set);
        if let Err(e) = save_token_file(&self.tokens_path, &tokens, &self.encryption_key) {
            tracing::warn!(
                "Failed to persist tokens to {}: {e}",
                self.tokens_path.display()
            );
        }
    }
}

// ── File I/O ──────────────────────────────────────────────────────────────────

fn load_token_file(path: &Path, key: &[u8; 32]) -> TokenFile {
    if !path.exists() {
        return TokenFile::default();
    }
    match std::fs::read_to_string(path) {
        Ok(s) => match decrypt_tokens(&s, key) {
            Ok(tf) => tf,
            Err(e) => {
                tracing::warn!(
                    "Could not decrypt {}: {e} — starting with empty token store",
                    path.display()
                );
                TokenFile::default()
            }
        },
        Err(e) => {
            tracing::warn!("Could not read {}: {e}", path.display());
            TokenFile::default()
        }
    }
}

fn save_token_file(path: &Path, tokens: &TokenFile, key: &[u8; 32]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let encrypted = encrypt_tokens(tokens, key)?;
    // Atomic write: write to a sibling .tmp file then rename.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, encrypted).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

// ── HTTP helpers ──────────────────────────────────────────────────────────────

/// Deserialize a token-endpoint response, surfacing OAuth error bodies as
/// human-readable errors rather than confusing "missing field" parse failures.
async fn parse_token_response(resp: reqwest::Response) -> Result<TokenResponse, String> {
    let status = resp.status();
    let body = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read token response body: {e}"))?;

    if !status.is_success() {
        // Try to extract a structured OAuth error first.
        if let Ok(err) = serde_json::from_slice::<OAuthErrorResponse>(&body) {
            let desc = err
                .error_description
                .filter(|d| !d.is_empty())
                .map(|d| format!(": {d}"))
                .unwrap_or_default();
            return Err(format!("OAuth error {}{}", err.error, desc));
        }
        return Err(format!(
            "Token endpoint returned HTTP {status}: {}",
            String::from_utf8_lossy(&body)
        ));
    }

    serde_json::from_slice::<TokenResponse>(&body)
        .map_err(|e| format!("Token response parse failed: {e}"))
}

// ── Crypto helpers ────────────────────────────────────────────────────────────

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce_pair() -> (String /* verifier */, String /* challenge */) {
    let mut bytes = [0u8; 64];
    rand::rng().fill_bytes(&mut bytes);
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

// ── Token encryption ──────────────────────────────────────────────────────────

/// Derive a 32-byte AES-256-GCM key from an arbitrary secret string via SHA-256.
pub fn derive_key(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

fn encrypt_tokens(tokens: &TokenFile, key: &[u8; 32]) -> Result<String, String> {
    let plaintext = serde_json::to_vec(tokens).map_err(|e| e.to_string())?;
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let cipher = Aes256Gcm::new_from_slice(key).expect("key is exactly 32 bytes");
    let nonce = nonce_bytes
        .as_slice()
        .try_into()
        .expect("nonce is exactly 12 bytes");
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_ref())
        .map_err(|e| format!("Encryption failed: {e}"))?;
    let payload = serde_json::json!({
        "v": 1,
        "n": URL_SAFE_NO_PAD.encode(nonce_bytes),
        "c": URL_SAFE_NO_PAD.encode(ciphertext),
    });
    serde_json::to_string(&payload).map_err(|e| e.to_string())
}

fn decrypt_tokens(data: &str, key: &[u8; 32]) -> Result<TokenFile, String> {
    #[derive(Deserialize)]
    struct EncryptedFile {
        v: u32,
        n: String,
        c: String,
    }
    let ef: EncryptedFile = serde_json::from_str(data).map_err(|e| format!("Parse failed: {e}"))?;
    if ef.v != 1 {
        return Err(format!("Unknown token file version: {}", ef.v));
    }
    let nonce_bytes = URL_SAFE_NO_PAD
        .decode(&ef.n)
        .map_err(|e| format!("Nonce decode failed: {e}"))?;
    let ciphertext = URL_SAFE_NO_PAD
        .decode(&ef.c)
        .map_err(|e| format!("Ciphertext decode failed: {e}"))?;
    let cipher = Aes256Gcm::new_from_slice(key).expect("key is exactly 32 bytes");
    let nonce: aes_gcm::Nonce<_> = nonce_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Nonce has wrong length".to_string())?;
    let plaintext = cipher
        .decrypt(&nonce, ciphertext.as_ref())
        .map_err(|e| format!("Decryption failed: {e}"))?;
    serde_json::from_slice(&plaintext).map_err(|e| format!("Token file parse failed: {e}"))
}
