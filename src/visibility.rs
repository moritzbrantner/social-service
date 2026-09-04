use serde::{Deserialize, Serialize};
use sqlx::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "social_visibility", rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Public,
    Private,
}

impl Visibility {
    pub fn can_view(self, owner_id: Uuid, viewer_id: Option<Uuid>) -> bool {
        matches!(self, Self::Public) || viewer_id == Some(owner_id)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::Visibility;

    #[test]
    fn public_content_is_visible_without_a_viewer() {
        assert!(Visibility::Public.can_view(Uuid::new_v4(), None));
    }

    #[test]
    fn private_content_is_visible_only_to_its_owner() {
        let owner = Uuid::new_v4();
        assert!(Visibility::Private.can_view(owner, Some(owner)));
        assert!(!Visibility::Private.can_view(owner, None));
        assert!(!Visibility::Private.can_view(owner, Some(Uuid::new_v4())));
    }
}
