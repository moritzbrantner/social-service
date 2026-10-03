use std::time::Duration;

use sqlx::PgPool;

use crate::features::FeatureSet;

pub const DEFAULT_READINESS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub features: FeatureSet,
    pub readiness_timeout: Duration,
}

impl AppState {
    pub fn new(pool: PgPool, features: FeatureSet) -> Self {
        Self {
            pool,
            features,
            readiness_timeout: DEFAULT_READINESS_TIMEOUT,
        }
    }

    pub fn with_readiness_timeout(mut self, readiness_timeout: Duration) -> Self {
        self.readiness_timeout = readiness_timeout;
        self
    }
}
