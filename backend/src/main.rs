use axum::Router;
use std::{net::SocketAddr, path::PathBuf};
use tower_http::{services::ServeDir, trace::TraceLayer};

mod api;
mod books;
mod build;
mod config;
mod deploy;
mod git;

/// Shared application state threaded through all request handlers.
#[derive(Clone)]
pub struct AppState {
    /// Live application configuration.
    pub config: config::SharedConfig,
    /// Path to the config file on disk (for saving updates).
    pub config_path: PathBuf,
    /// Path to the cloned book monorepo on disk (cached from config).
    pub data_dir: PathBuf,
    /// Catalogue of books scanned from the repo, refreshed on pull.
    pub catalogue: books::SharedCatalogue,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend=debug,tower_http=debug".into()),
        )
        .init();

    let spa_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "../frontend/build".to_string());

    let state = build_state();

    let api_router = api::router();

    let app = Router::new()
        .nest("/api", api_router)
        .fallback_service(spa_fallback(spa_dir))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn build_state() -> AppState {
    let config_path = PathBuf::from(
        std::env::var("CONFIG_FILE").unwrap_or_else(|_| "config/config.json".to_string()),
    );

    let cfg = config::load_or_create(&config_path);
    let data_dir = cfg.data_dir.clone();
    let mut initial_pull: Option<chrono::DateTime<chrono::Utc>> = None;

    // Attempt initial git sync (best-effort: a missing/empty config is fine,
    // the user can configure via the UI then hit Pull).
    if !cfg.forgejo.pat.is_empty() && !cfg.forgejo.url.is_empty() {
        let repo_url = format!(
            "{}/{}",
            cfg.forgejo.url.trim_end_matches('/'),
            cfg.forgejo.repo
        );
        if let Err(e) = git::sync_repo(&repo_url, &cfg.forgejo.pat, &data_dir) {
            tracing::error!("Initial git sync failed: {e}");
        } else {
            initial_pull = Some(chrono::Utc::now());
        }
    } else {
        tracing::warn!("Forgejo config incomplete — skipping initial git sync");
    }

    let scanned = books::scan(&data_dir);
    tracing::info!("Found {} book(s) in {}", scanned.len(), data_dir.display());

    AppState {
        config_path,
        data_dir,
        catalogue: std::sync::Arc::new(std::sync::RwLock::new(books::Catalogue {
            last_pull: initial_pull,
            books: scanned,
        })),
        config: std::sync::Arc::new(std::sync::RwLock::new(cfg)),
    }
}

/// Serves static files from `dir`; falls back to `index.html` for SPA routing.
fn spa_fallback(dir: String) -> Router {
    let serve_dir = ServeDir::new(dir.clone()).not_found_service(
        tower_http::services::ServeFile::new(format!("{dir}/index.html")),
    );

    Router::new().fallback_service(serve_dir)
}
