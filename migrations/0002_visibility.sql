CREATE TYPE social_visibility AS ENUM ('public', 'private');

ALTER TABLE profiles
    ADD COLUMN visibility social_visibility NOT NULL DEFAULT 'public';

ALTER TABLE posts
    ADD COLUMN visibility social_visibility NOT NULL DEFAULT 'public';

CREATE INDEX profiles_app_visibility_idx ON profiles (app_id, visibility, user_id);
CREATE INDEX posts_app_visibility_created_idx ON posts (app_id, visibility, created_at DESC);
