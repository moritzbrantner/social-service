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
    Moderation,
}

impl Feature {
    pub const ALL: [Self; 7] = [
        Self::Profiles,
        Self::Media,
        Self::Posts,
        Self::Comments,
        Self::Follows,
        Self::Chat,
        Self::Moderation,
    ];

    fn parse(value: &str) -> Option<Self> {
        match value {
            "profiles" => Some(Self::Profiles),
            "media" => Some(Self::Media),
            "posts" => Some(Self::Posts),
            "comments" => Some(Self::Comments),
            "follows" => Some(Self::Follows),
            "chat" => Some(Self::Chat),
            "moderation" => Some(Self::Moderation),
            _ => None,
        }
    }

    pub const fn requires(self) -> &'static [Self] {
        match self {
            Self::Profiles => &[],
            Self::Media | Self::Posts | Self::Follows | Self::Chat | Self::Moderation => {
                &[Self::Profiles]
            }
            Self::Comments => &[Self::Posts],
        }
    }

    pub const fn integrates_with(self) -> &'static [Self] {
        match self {
            Self::Profiles | Self::Posts | Self::Chat => &[Self::Media],
            Self::Moderation => &[
                Self::Media,
                Self::Posts,
                Self::Comments,
                Self::Follows,
                Self::Chat,
            ],
            Self::Media | Self::Comments | Self::Follows => &[],
        }
    }

    pub const fn conflicts(self) -> &'static [Self] {
        &[]
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
            Self::Moderation => "moderation",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone)]
pub struct FeatureSet {
    implemented: HashSet<Feature>,
    deployment_supported: HashSet<Feature>,
    app_requested: HashSet<Feature>,
    app_effective: HashSet<Feature>,
}

impl FeatureSet {
    pub fn from_csv(value: &str) -> Result<Self, FeatureError> {
        let implemented = Feature::ALL.into_iter().collect::<HashSet<_>>();
        let app_requested = parse_csv(value)?;
        let deployment_supported = resolve(app_requested.clone(), &implemented)?;
        let app_effective = resolve(app_requested.clone(), &deployment_supported)?;

        Ok(Self {
            implemented,
            deployment_supported,
            app_requested,
            app_effective,
        })
    }

    pub fn for_app_csv(&self, value: &str) -> Result<Self, FeatureError> {
        let app_requested = parse_csv(value)?;
        let app_effective = resolve(app_requested.clone(), &self.deployment_supported)?;
        Ok(Self {
            implemented: self.implemented.clone(),
            deployment_supported: self.deployment_supported.clone(),
            app_requested,
            app_effective,
        })
    }

    pub fn enabled(&self) -> Vec<Feature> {
        ordered(&self.app_effective)
    }

    pub fn implemented(&self) -> Vec<Feature> {
        ordered(&self.implemented)
    }

    pub fn deployment_supported(&self) -> Vec<Feature> {
        ordered(&self.deployment_supported)
    }

    pub fn app_requested(&self) -> Vec<Feature> {
        ordered(&self.app_requested)
    }

    pub fn effective(&self) -> Vec<Feature> {
        ordered(&self.app_effective)
    }

    pub fn is_enabled(&self, feature: Feature) -> bool {
        self.app_effective.contains(&feature)
    }

    pub fn require(&self, feature: Feature) -> Result<(), ApiError> {
        if self.is_enabled(feature) {
            Ok(())
        } else {
            Err(ApiError::FeatureDisabled(feature))
        }
    }
}

fn parse_csv(value: &str) -> Result<HashSet<Feature>, FeatureError> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|raw| Feature::parse(raw).ok_or_else(|| FeatureError::Unknown(raw.to_owned())))
        .collect()
}

fn resolve(
    requested: HashSet<Feature>,
    available: &HashSet<Feature>,
) -> Result<HashSet<Feature>, FeatureError> {
    for feature in Feature::ALL {
        if requested.contains(&feature) && !available.contains(&feature) {
            return Err(FeatureError::Unsupported(feature));
        }
    }

    let mut effective = requested;
    loop {
        let mut changed = false;
        for feature in Feature::ALL {
            if !effective.contains(&feature) {
                continue;
            }
            for dependency in feature.requires() {
                if !available.contains(dependency) {
                    return Err(FeatureError::UnsupportedDependency {
                        feature,
                        dependency: *dependency,
                    });
                }
                changed |= effective.insert(*dependency);
            }
        }
        if !changed {
            break;
        }
    }

    for feature in Feature::ALL {
        if !effective.contains(&feature) {
            continue;
        }
        for conflict in feature.conflicts() {
            if effective.contains(conflict) {
                return Err(FeatureError::Conflict {
                    feature,
                    conflict: *conflict,
                });
            }
        }
    }

    Ok(effective)
}

fn ordered(features: &HashSet<Feature>) -> Vec<Feature> {
    Feature::ALL
        .into_iter()
        .filter(|feature| features.contains(feature))
        .collect()
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FeatureError {
    #[error("unknown social feature `{0}`")]
    Unknown(String),
    #[error("feature `{0}` is not supported by this deployment")]
    Unsupported(Feature),
    #[error("feature `{feature}` requires unsupported feature `{dependency}`")]
    UnsupportedDependency {
        feature: Feature,
        dependency: Feature,
    },
    #[error("feature `{feature}` conflicts with `{conflict}`")]
    Conflict { feature: Feature, conflict: Feature },
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
    fn resolves_transitive_dependencies() {
        let features = FeatureSet::from_csv("comments").expect("dependencies resolve");
        assert_eq!(features.app_requested(), vec![Feature::Comments]);
        assert_eq!(
            features.effective(),
            vec![Feature::Profiles, Feature::Posts, Feature::Comments]
        );
    }

    #[test]
    fn moderation_resolves_profiles_without_enabling_social_surfaces() {
        let features = FeatureSet::from_csv("moderation").expect("moderation should resolve");
        assert_eq!(features.app_requested(), vec![Feature::Moderation]);
        assert_eq!(
            features.effective(),
            vec![Feature::Profiles, Feature::Moderation]
        );
    }

    #[test]
    fn app_requests_are_bounded_by_deployment_support() {
        let deployment =
            FeatureSet::from_csv("comments,follows").expect("valid deployment features");
        let app = deployment
            .for_app_csv("comments")
            .expect("supported app subset");
        assert_eq!(app.app_requested(), vec![Feature::Comments]);
        assert_eq!(
            app.effective(),
            vec![Feature::Profiles, Feature::Posts, Feature::Comments]
        );
        assert_eq!(
            app.deployment_supported(),
            vec![
                Feature::Profiles,
                Feature::Posts,
                Feature::Comments,
                Feature::Follows
            ]
        );
    }

    #[test]
    fn rejects_app_requests_outside_deployment_support() {
        let deployment = FeatureSet::from_csv("profiles").expect("valid deployment features");
        assert_eq!(
            deployment
                .for_app_csv("chat")
                .expect_err("chat is outside deployment support"),
            FeatureError::Unsupported(Feature::Chat)
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
