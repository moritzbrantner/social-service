use std::{env, net::SocketAddr};

use thiserror::Error;

use crate::features::{FeatureError, FeatureSet};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: SocketAddr,
    pub features: FeatureSet,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url =
            env::var("DATABASE_URL").map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        let bind = env::var("SOCIAL_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
            .parse()
            .map_err(ConfigError::InvalidBind)?;
        let features = FeatureSet::from_csv(
            &env::var("SOCIAL_FEATURES")
                .unwrap_or_else(|_| "profiles,media,posts,comments,follows,chat".to_owned()),
        )?;

        Ok(Self {
            database_url,
            bind,
            features,
        })
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("SOCIAL_BIND is invalid: {0}")]
    InvalidBind(std::net::AddrParseError),
    #[error(transparent)]
    Features(#[from] FeatureError),
}
