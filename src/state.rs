use sqlx::PgPool;

use crate::features::FeatureSet;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub features: FeatureSet,
}

impl AppState {
    pub fn new(pool: PgPool, features: FeatureSet) -> Self {
        Self { pool, features }
    }
}
