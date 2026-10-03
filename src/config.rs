use std::{env, net::SocketAddr, time::Duration};

use thiserror::Error;

use crate::features::{FeatureError, FeatureSet};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: SocketAddr,
    pub features: FeatureSet,
    pub readiness_timeout: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url =
            env::var("DATABASE_URL").map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        let bind = env::var("SOCIAL_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
            .parse()
            .map_err(ConfigError::InvalidBind)?;
        let features = FeatureSet::from_csv(&env::var("SOCIAL_FEATURES").unwrap_or_else(|_| {
            "profiles,media,posts,comments,reactions,votes,follows,follow_requests,saves,blocks,mutes,groups,chat"
                .to_owned()
        }))?;
        let readiness_timeout_ms = env::var("SOCIAL_READINESS_TIMEOUT_MS")
            .unwrap_or_else(|_| "2000".to_owned())
            .parse::<u64>()
            .map_err(ConfigError::InvalidReadinessTimeout)?;
        if readiness_timeout_ms == 0 {
            return Err(ConfigError::ZeroReadinessTimeout);
        }

        Ok(Self {
            database_url,
            bind,
            features,
            readiness_timeout: Duration::from_millis(readiness_timeout_ms),
        })
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("SOCIAL_BIND is invalid: {0}")]
    InvalidBind(std::net::AddrParseError),
    #[error("SOCIAL_READINESS_TIMEOUT_MS is invalid: {0}")]
    InvalidReadinessTimeout(std::num::ParseIntError),
    #[error("SOCIAL_READINESS_TIMEOUT_MS must be greater than zero")]
    ZeroReadinessTimeout,
    #[error(transparent)]
    Features(#[from] FeatureError),
}
