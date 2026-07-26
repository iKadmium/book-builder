use axum::{Router, routing::get};

use crate::AppState;

mod books;
mod config;
mod oauth;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .merge(books::router())
        .merge(config::router())
        .merge(oauth::router())
}

async fn health() -> &'static str {
    "ok"
}
