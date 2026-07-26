use axum::{Json, Router, extract::State, http::StatusCode, routing::get};

use crate::{AppState, config};

pub fn router() -> Router<AppState> {
    Router::new().route("/config", get(get_config).put(put_config))
}

async fn get_config(State(state): State<AppState>) -> Result<Json<config::Config>, StatusCode> {
    let cfg = state.config.read().map_err(|_| {
        tracing::error!("config lock poisoned");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(cfg.redacted()))
}

async fn put_config(
    State(state): State<AppState>,
    Json(mut new_config): Json<config::Config>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Restore any secrets the browser left blank (sent as empty strings).
    {
        let existing = state
            .config
            .read()
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?;
        new_config.apply_secrets_from(&existing);
    }

    config::save(&new_config, &state.config_path).map_err(|e| {
        tracing::error!("Failed to save config: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, e)
    })?;

    if let Ok(mut cfg) = state.config.write() {
        *cfg = new_config;
    }

    Ok(StatusCode::NO_CONTENT)
}
