use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use uuid::Uuid;

use crate::{
    auth::RequestContext, error::ApiError, features::Feature, groups::Group, models::LimitQuery,
    state::AppState,
};

use super::support::load_group;

pub async fn list_groups(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<Group>>, ApiError> {
    state.features.require(Feature::Groups)?;
    let context = RequestContext::from_headers(&headers)?;
    let group_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT g.id FROM groups g JOIN group_members gm ON gm.app_id = g.app_id AND gm.group_id = g.id WHERE g.app_id = $1 AND gm.user_id = $2 ORDER BY g.updated_at DESC, g.id ASC LIMIT $3",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;

    let mut groups = Vec::with_capacity(group_ids.len());
    for group_id in group_ids {
        groups.push(
            load_group(&state, context.app_id.0, group_id, context.user_id.0).await?,
        );
    }
    Ok(Json(groups))
}

pub async fn get_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<Uuid>,
) -> Result<Json<Group>, ApiError> {
    state.features.require(Feature::Groups)?;
    let context = RequestContext::from_headers(&headers)?;
    Ok(Json(
        load_group(&state, context.app_id.0, group_id, context.user_id.0).await?,
    ))
}
