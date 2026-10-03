//! Disposable loopback fixture for the repository-owned load:smoke scenario.
use std::{env, error::Error, fs, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use social_service::{app, features::FeatureSet, state::AppState};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

const APP: Uuid = Uuid::from_u128(1);
const VIEWER: Uuid = Uuid::from_u128(2);
const AUTHOR: Uuid = Uuid::from_u128(3);

async fn prepare() -> Result<AppState, Box<dyn Error>> {
    let url = env::var("SOCIAL_LOAD_DATABASE_URL")?;
    let port = url
        .strip_prefix("postgres://social:social@127.0.0.1:")
        .and_then(|value| value.strip_suffix("/social"))
        .ok_or("load fixture requires its private loopback database")?
        .parse::<u16>()?;
    if port == 0 {
        return Err("invalid private database port".into());
    }
    env::var("RUNTIME_PROFILER_PORT_FILE")?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&url)
        .await?;
    sqlx::migrate!().run(&pool).await?;
    for (user, name) in [(VIEWER, "Load viewer"), (AUTHOR, "Load author")] {
        sqlx::query("INSERT INTO profiles (app_id,user_id,display_name,created_at,updated_at) VALUES ($1,$2,$3,'2020-01-01T00:00:00Z','2020-01-01T00:00:00Z')")
            .bind(APP).bind(user).bind(name).execute(&pool).await?;
    }
    sqlx::query("INSERT INTO follows (app_id,follower_id,followed_id,created_at) VALUES ($1,$2,$3,'2020-01-01T00:00:00Z')")
        .bind(APP).bind(VIEWER).bind(AUTHOR).execute(&pool).await?;
    for index in 0..8_u128 {
        sqlx::query("INSERT INTO posts (id,app_id,author_id,body,created_at,updated_at) VALUES ($1,$2,$3,$4,'2020-01-01T00:00:00Z','2020-01-01T00:00:00Z')")
            .bind(Uuid::from_u128(100 + index)).bind(APP).bind(AUTHOR)
            .bind(format!("Deterministic load post {index}")).execute(&pool).await?;
    }
    let state = AppState::new(pool, FeatureSet::from_csv("profiles,posts,follows")?);
    // Verify real domain responses before allowing the measurement engine to start.
    for (path, timeline) in [
        ("/v1/timeline?limit=8", true),
        ("/v1/profiles/00000000-0000-0000-0000-000000000003", false),
    ] {
        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("x-app-id", APP.to_string())
                    .header("x-user-id", VIEWER.to_string())
                    .body(Body::empty())?,
            )
            .await?;
        if response.status() != StatusCode::OK {
            return Err("seeded API did not succeed".into());
        }
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
        if timeline {
            if body.as_array().map(Vec::len) != Some(8) {
                return Err("seeded timeline must contain eight posts".into());
            }
        } else if body["displayName"] != "Load author" {
            return Err("seeded profile mismatch".into());
        }
    }
    Ok(state)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let state = tokio::time::timeout(Duration::from_secs(20), prepare()).await??;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    fs::write(
        env::var("RUNTIME_PROFILER_PORT_FILE")?,
        listener.local_addr()?.port().to_string(),
    )?;
    axum::serve(listener, app(state)).await?;
    Ok(())
}
