use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use uuid::Uuid;

use crate::{
    auth::RequestContext, error::ApiError, features::Feature, groups::GroupRole, state::AppState,
};

use super::support::{append_membership_event, lock_actor_role, sync_linked_chat, touch_group};

pub async fn leave_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Groups)?;
    let context = RequestContext::from_headers(&headers)?;

    // Leaving is an exit operation, not participation. It remains available even when
    // the caller is group-restricted or the group is hidden/removed by moderation.
    let mut transaction = state.pool.begin().await?;
    let actor_role = lock_actor_role(
        &mut transaction,
        context.app_id.0,
        group_id,
        context.user_id.0,
    )
    .await?;
    if actor_role == GroupRole::Owner {
        return Err(ApiError::BadRequest(
            "transfer group ownership before leaving".to_owned(),
        ));
    }

    append_membership_event(
        &mut transaction,
        context.app_id.0,
        group_id,
        context.user_id.0,
        "left",
        context.user_id.0,
        (Some(actor_role), None),
    )
    .await?;
    sqlx::query("DELETE FROM group_members WHERE app_id = $1 AND group_id = $2 AND user_id = $3")
        .bind(context.app_id.0)
        .bind(group_id)
        .bind(context.user_id.0)
        .execute(&mut *transaction)
        .await?;
    sync_linked_chat(&mut transaction, context.app_id.0, group_id).await?;
    touch_group(&mut transaction, context.app_id.0, group_id).await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
