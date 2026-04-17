pub mod api;
pub mod htmx;
pub mod pages;

use crate::state::AppState;
use axum::Router;
use std::sync::Arc;
use tower_http::set_header::SetResponseHeaderLayer;

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(pages::routes())
        .merge(api::routes())
        .merge(htmx::routes())
        .with_state(state)
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            axum::http::HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            axum::http::HeaderValue::from_static("nosniff"),
        ))
}
