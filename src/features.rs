use std::{collections::HashSet, fmt};

use serde::Serialize;
use thiserror::Error;

use crate::error::ApiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Feature {
    Profiles,
    Media,
    Posts,
    Comments,
    Follows,
    Chat,
}

impl Feature {
    pub const ALL: [Self; 6] = [
        Self::Profiles,
        Self::Media,
        Self::Posts,
        Self::Comments,
        Self::Follows,
        Self::Chat,
    ];

    fn parse(value: &str) -> Option<Self> {
        match value {
            "profiles" => Some(Self::Profiles),
            "media" => Some(Self::Media),
            "posts" => Some(Self::Posts),
            "comments" => Some(Self::Comments),
            "follows" => Some(Self::Follows),
            "chat" => Some(Self::Chat),
            _ => None,
        }
    }
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Profiles => "profiles",
            Self::Media => "media",
            Self::Posts => "posts",
            Self::Comments => "comments",
            Self::Follows => "follows",
            Self::Chat => "chat",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone)]
pub struct FeatureSet(HashSet<Feature>);

impl FeatureSet {
    pub fn from_csv(value: &str) -> Result<Self, FeatureError> {
        let mut enabled = HashSet::new();
        for raw in value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let feature =
                Feature::parse(raw).ok_or_else(|| FeatureError::Unknown(raw.to_owned()))?;
            enabled.insert(feature);
        }
        let set = Self(enabled);
        set.validate()?;
        Ok(set)
    }

    pub fn enabled(&self) -> Vec<Feature> {
        Feature::ALL
            .into_iter()
            .filter(|feature| self.0.contains(feature))
            .collect()
    }

    pub fn require(&self, feature: Feature) -> Result<(), ApiError> {
        if self.0.contains(&feature) {
            Ok(())
        } else {
            Err(ApiError::FeatureDisabled(feature))
        }
    }

    fn validate(&self) -> Result<(), FeatureError> {
        for (feature, dependency) in [
            (Feature::Media, Feature::Profiles),
            (Feature::Posts, Feature::Profiles),
            (Feature::Comments, Feature::Posts),
            (Feature::Follows, Feature::Profiles),
            (Feature::Chat, Feature::Profiles),
        ] {
            if self.0.contains(&feature) && !self.0.contains(&dependency) {
                return Err(FeatureError::MissingDependency {
                    feature,
                    dependency,
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FeatureError {
    #[error("unknown social feature `{0}`")]
    Unknown(String),
    #[error("feature `{feature}` requires `{dependency}`")]
    MissingDependency {
        feature: Feature,
        dependency: Feature,
    },
}

#[cfg(test)]
mod tests {
    use super::{Feature, FeatureError, FeatureSet};

    #[test]
    fn parses_enabled_features_in_canonical_order() {
        let features = FeatureSet::from_csv("chat,profiles").expect("valid feature set");
        assert_eq!(features.enabled(), vec![Feature::Profiles, Feature::Chat]);
    }

    #[test]
    fn rejects_missing_dependencies() {
        let error = FeatureSet::from_csv("comments").expect_err("comments require posts");
        assert_eq!(
            error,
            FeatureError::MissingDependency {
                feature: Feature::Comments,
                dependency: Feature::Posts,
            }
        );
    }

    #[test]
    fn rejects_unknown_features() {
        assert_eq!(
            FeatureSet::from_csv("profiles,telepathy").expect_err("unknown feature must fail"),
            FeatureError::Unknown("telepathy".to_owned())
        );
    }
}
