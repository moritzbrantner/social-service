mod chat;
mod media;
mod posts;
mod profiles;

use axum::{
    Json, Router,
    routing::{get, post, put},
};
use serde::Serialize;

use crate::{features::Feature, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/profiles/{user_id}", get(profiles::get_profile))
        .route("/profiles/me", put(profiles::upsert_profile))
        .route("/media", post(media::register_media))
        .route("/posts", post(posts::create_post))
        .route(
            "/posts/{post_id}",
            get(posts::get_post).delete(posts::delete_post),
        )
        .route(
            "/posts/{post_id}/comments",
            get(posts::list_comments).post(posts::create_comment),
        )
        .route(
            "/follows/{user_id}",
            put(posts::follow_user).delete(posts::unfollow_user),
        )
        .route("/timeline", get(posts::timeline))
        .route(
            "/conversations",
            post(chat::create_conversation).get(chat::list_conversations),
        )
        .route(
            "/conversations/{conversation_id}/messages",
            get(chat::list_messages).post(chat::create_message),
        )
}

pub async fn health() -> &'static str {
    "ok"
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeaturesResponse {
    enabled: Vec<Feature>,
}

pub async fn features(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<FeaturesResponse> {
    Json(FeaturesResponse {
        enabled: state.features.enabled(),
    })
}
