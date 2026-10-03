use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::ApiError;

pub async fn lock_user_pair(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    left_id: Uuid,
    right_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query("SELECT social_lock_user_pair($1, $2, $3)")
        .bind(app_id)
        .bind(left_id)
        .bind(right_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

pub async fn lock_users(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    user_ids: &[Uuid],
) -> Result<(), ApiError> {
    if user_ids.is_empty() {
        return Ok(());
    }

    let mut user_ids = user_ids.to_vec();
    user_ids.sort_unstable();
    user_ids.dedup();
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended($1::text || ':user:' || locked_user.user_id::text, 0)) FROM unnest($2::uuid[]) AS locked_user(user_id) ORDER BY locked_user.user_id ASC",
    )
    .bind(app_id)
    .bind(user_ids)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
