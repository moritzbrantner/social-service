use std::time::Duration;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use social_service::{app, features::FeatureSet, relationships::lock_users, state::AppState};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires PostgreSQL"]
async fn block_creation_serializes_conversation_reaction_and_vote_writes() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("PostgreSQL should be reachable");
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("migrations should apply");

    let state = AppState::new(
        pool.clone(),
        FeatureSet::from_csv("chat,blocks,reactions,votes")
            .expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let alice = Uuid::new_v4();
    let bob = Uuid::new_v4();

    for (user_id, display_name) in [(alice, "Alice"), (bob, "Bob")] {
        let response = send(
            &state,
            Method::PUT,
            "/v1/profiles/me",
            app_id,
            user_id,
            Some(json!({ "displayName": display_name })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    let post = json_body(
        send(
            &state,
            Method::POST,
            "/v1/posts",
            app_id,
            bob,
            Some(json!({ "body": "race target" })),
        )
        .await,
    )
    .await;
    let post_id = Uuid::parse_str(post["id"].as_str().expect("post id")).expect("post UUID");

    let profileless_user = Uuid::new_v4();
    for uri in [
        format!("/v1/reactions/post/{post_id}/like"),
        format!("/v1/votes/post/{post_id}/up"),
    ] {
        let response = send(&state, Method::PUT, &uri, app_id, profileless_user, None).await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "feedback writes require an established social profile instead of leaking a database foreign-key error"
        );
    }

    let mut block_tx = pool.begin().await.expect("block transaction");
    lock_users(&mut block_tx, app_id, &[alice, bob])
        .await
        .expect("block user locks");
    lock_pair(&mut block_tx, app_id, alice, bob).await;
    let request_state = state.clone();
    let conversation = tokio::spawn(async move {
        send(
            &request_state,
            Method::POST,
            "/v1/conversations",
            app_id,
            alice,
            Some(json!({ "memberIds": [bob] })),
        )
        .await
    });
    wait_for_lock_waiter(&pool).await;
    insert_block(&mut block_tx, app_id, bob, alice).await;
    block_tx.commit().await.expect("block commit");
    let response = conversation
        .await
        .expect("conversation request should complete");
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a conversation that passed its initial checks must still observe a concurrently committed block"
    );
    let conversation_count =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM conversations WHERE app_id = $1")
            .bind(app_id)
            .fetch_one(&pool)
            .await
            .expect("conversation count");
    assert_eq!(conversation_count, 0);
    clear_block(&pool, app_id, bob, alice).await;

    let mut block_tx = pool.begin().await.expect("block transaction");
    lock_users(&mut block_tx, app_id, &[alice, bob])
        .await
        .expect("block user locks");
    lock_pair(&mut block_tx, app_id, alice, bob).await;
    let request_state = state.clone();
    let reaction = tokio::spawn(async move {
        send(
            &request_state,
            Method::PUT,
            &format!("/v1/reactions/post/{post_id}/like"),
            app_id,
            alice,
            None,
        )
        .await
    });
    wait_for_lock_waiter(&pool).await;
    insert_block(&mut block_tx, app_id, bob, alice).await;
    block_tx.commit().await.expect("block commit");
    let response = reaction.await.expect("reaction request should complete");
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "a reaction must not be recreated after a concurrent block commits"
    );
    let reaction_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM reactions WHERE app_id = $1 AND target_type = 'post' AND target_id = $2 AND user_id = $3",
    )
    .bind(app_id)
    .bind(post_id)
    .bind(alice)
    .fetch_one(&pool)
    .await
    .expect("reaction count");
    assert_eq!(reaction_count, 0);
    clear_block(&pool, app_id, bob, alice).await;

    let mut block_tx = pool.begin().await.expect("block transaction");
    lock_users(&mut block_tx, app_id, &[alice, bob])
        .await
        .expect("block user locks");
    lock_pair(&mut block_tx, app_id, alice, bob).await;
    let request_state = state.clone();
    let vote = tokio::spawn(async move {
        send(
            &request_state,
            Method::PUT,
            &format!("/v1/votes/post/{post_id}/up"),
            app_id,
            alice,
            None,
        )
        .await
    });
    wait_for_lock_waiter(&pool).await;
    insert_block(&mut block_tx, app_id, bob, alice).await;
    block_tx.commit().await.expect("block commit");
    let response = vote.await.expect("vote request should complete");
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "a vote must not be recreated after a concurrent block commits"
    );
    let vote_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM votes WHERE app_id = $1 AND target_type = 'post' AND target_id = $2 AND user_id = $3",
    )
    .bind(app_id)
    .bind(post_id)
    .bind(alice)
    .fetch_one(&pool)
    .await
    .expect("vote count");
    assert_eq!(vote_count, 0);

    let hundred_members = (1_u128..=100).map(Uuid::from_u128).collect::<Vec<_>>();
    let mut bounded_lock_tx = pool.begin().await.expect("bounded lock transaction");
    lock_users(&mut bounded_lock_tx, app_id, &hundred_members)
        .await
        .expect("100-member user lock set");
    let advisory_lock_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM pg_locks WHERE pid = pg_backend_pid() AND locktype = 'advisory' AND granted",
    )
    .fetch_one(&mut *bounded_lock_tx)
    .await
    .expect("advisory lock count");
    assert_eq!(
        advisory_lock_count, 100,
        "a 100-member conversation must consume one advisory lock per user, not one per pair"
    );
    bounded_lock_tx
        .rollback()
        .await
        .expect("bounded lock rollback");
}

async fn lock_pair(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    app_id: Uuid,
    left_id: Uuid,
    right_id: Uuid,
) {
    sqlx::query("SELECT social_lock_user_pair($1, $2, $3)")
        .bind(app_id)
        .bind(left_id)
        .bind(right_id)
        .execute(&mut **transaction)
        .await
        .expect("pair lock");
}

async fn insert_block(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    app_id: Uuid,
    blocker_id: Uuid,
    blocked_id: Uuid,
) {
    sqlx::query(
        "INSERT INTO user_blocks (app_id, blocker_id, blocked_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(app_id)
    .bind(blocker_id)
    .bind(blocked_id)
    .execute(&mut **transaction)
    .await
    .expect("block insert");
}

async fn clear_block(pool: &PgPool, app_id: Uuid, blocker_id: Uuid, blocked_id: Uuid) {
    sqlx::query(
        "DELETE FROM user_blocks WHERE app_id = $1 AND blocker_id = $2 AND blocked_id = $3",
    )
    .bind(app_id)
    .bind(blocker_id)
    .bind(blocked_id)
    .execute(pool)
    .await
    .expect("block cleanup");
}

async fn wait_for_lock_waiter(pool: &PgPool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname = current_database() AND pid <> pg_backend_pid() AND wait_event_type = 'Lock' AND (query LIKE '%social_lock_user_pair%' OR query LIKE '%:user:%'))",
            )
            .fetch_one(pool)
            .await
            .expect("lock wait state");
            if waiting {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("request should reach a shared safety lock before the block commits");
}

async fn send(
    state: &AppState,
    method: Method,
    uri: &str,
    app_id: Uuid,
    user_id: Uuid,
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-app-id", app_id.to_string())
        .header("x-user-id", user_id.to_string());
    let body = if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };

    app(state.clone())
        .oneshot(builder.body(body).expect("request should build"))
        .await
        .expect("router should respond")
}

async fn json_body(response: Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body should be readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response should contain JSON")
}
