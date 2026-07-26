use axum::{Router, routing::get};

use crate::AppState;

mod books;
mod config;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .merge(books::router())
        .merge(config::router())
}

async fn health() -> &'static str {
    "ok"
}
