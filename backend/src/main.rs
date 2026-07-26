use axum::Router;
use std::net::SocketAddr;
use tower_http::{services::ServeDir, trace::TraceLayer};

mod api;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend=debug,tower_http=debug".into()),
        )
        .init();

    let spa_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "../frontend/build".to_string());

    let api_router = api::router();

    let app = Router::new()
        .nest("/api", api_router)
        .fallback_service(spa_fallback(spa_dir))
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Serves static files from `dir`; falls back to `index.html` for SPA routing.
fn spa_fallback(dir: String) -> Router {
    let serve_dir = ServeDir::new(dir.clone()).not_found_service(
        tower_http::services::ServeFile::new(format!("{dir}/index.html")),
    );

    Router::new().fallback_service(serve_dir)
}
