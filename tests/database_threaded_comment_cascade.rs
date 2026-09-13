use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires PostgreSQL"]
async fn deleting_a_post_cascades_through_threaded_comments() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("PostgreSQL should be reachable");
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("migrations should apply");

    let app_id = Uuid::new_v4();
    let author_id = Uuid::new_v4();
    let post_id = Uuid::new_v4();
    let root_id = Uuid::new_v4();
    let reply_id = Uuid::new_v4();

    sqlx::query("INSERT INTO posts (id, app_id, author_id, body) VALUES ($1, $2, $3, $4)")
        .bind(post_id)
        .bind(app_id)
        .bind(author_id)
        .bind("post with a comment tree")
        .execute(&pool)
        .await
        .expect("post should insert");
    sqlx::query(
        "INSERT INTO comments (id, app_id, post_id, author_id, body) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(root_id)
    .bind(app_id)
    .bind(post_id)
    .bind(author_id)
    .bind("root")
    .execute(&pool)
    .await
    .expect("root comment should insert");
    sqlx::query(
        "INSERT INTO comments (id, app_id, post_id, parent_comment_id, author_id, body) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(reply_id)
    .bind(app_id)
    .bind(post_id)
    .bind(root_id)
    .bind(author_id)
    .bind("reply")
    .execute(&pool)
    .await
    .expect("reply should insert");

    sqlx::query("DELETE FROM posts WHERE app_id = $1 AND id = $2")
        .bind(app_id)
        .bind(post_id)
        .execute(&pool)
        .await
        .expect("post deletion should cascade through the whole comment tree");

    let remaining = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM comments WHERE app_id = $1 AND post_id = $2",
    )
    .bind(app_id)
    .bind(post_id)
    .fetch_one(&pool)
    .await
    .expect("comment count should load");
    assert_eq!(remaining, 0);
}
