pub mod auth;
pub mod config;
pub mod error;
pub mod features;
pub mod models;
pub mod moderation;
pub mod routes;
pub mod state;
pub mod visibility;

use axum::{Router, routing::get};
use state::AppState;
use tower_http::trace::TraceLayer;

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health))
        .route("/v1/features", get(routes::features))
        .nest("/v1", routes::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
