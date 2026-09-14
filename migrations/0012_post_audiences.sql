CREATE TYPE social_post_audience AS ENUM ('public', 'owner_only', 'approved_followers');

ALTER TABLE posts
    ADD COLUMN audience social_post_audience;

UPDATE posts
SET audience = CASE visibility
    WHEN 'public' THEN 'public'::social_post_audience
    WHEN 'private' THEN 'owner_only'::social_post_audience
END;

ALTER TABLE posts
    ALTER COLUMN audience SET DEFAULT 'public'::social_post_audience,
    ALTER COLUMN audience SET NOT NULL;

ALTER TABLE posts
    ADD CONSTRAINT posts_visibility_matches_audience CHECK (
        (audience = 'public' AND visibility = 'public')
        OR (audience IN ('owner_only', 'approved_followers') AND visibility = 'private')
    );

CREATE INDEX posts_app_author_audience_created_idx
    ON posts (app_id, author_id, audience, created_at DESC);
