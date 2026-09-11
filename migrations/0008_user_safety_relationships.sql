CREATE TABLE user_blocks (
    app_id UUID NOT NULL,
    blocker_id UUID NOT NULL,
    blocked_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (app_id, blocker_id, blocked_id),
    CHECK (blocker_id <> blocked_id),
    FOREIGN KEY (app_id, blocker_id) REFERENCES profiles(app_id, user_id) ON DELETE CASCADE,
    FOREIGN KEY (app_id, blocked_id) REFERENCES profiles(app_id, user_id) ON DELETE CASCADE
);

CREATE INDEX user_blocks_blocked_idx
    ON user_blocks (app_id, blocked_id, blocker_id);

CREATE TABLE user_mutes (
    app_id UUID NOT NULL,
    muter_id UUID NOT NULL,
    muted_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (app_id, muter_id, muted_id),
    CHECK (muter_id <> muted_id),
    FOREIGN KEY (app_id, muter_id) REFERENCES profiles(app_id, user_id) ON DELETE CASCADE,
    FOREIGN KEY (app_id, muted_id) REFERENCES profiles(app_id, user_id) ON DELETE CASCADE
);

CREATE INDEX user_mutes_muted_idx
    ON user_mutes (app_id, muted_id, muter_id);

CREATE FUNCTION social_lock_user_pair(p_app_id UUID, p_left_id UUID, p_right_id UUID)
RETURNS VOID
LANGUAGE plpgsql
VOLATILE
AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(
        hashtextextended(
            p_app_id::text || ':' || LEAST(p_left_id, p_right_id)::text || ':' || GREATEST(p_left_id, p_right_id)::text,
            0
        )
    );
END;
$$;

CREATE FUNCTION social_users_blocked(p_app_id UUID, p_left_id UUID, p_right_id UUID)
RETURNS BOOLEAN
LANGUAGE sql
STABLE
PARALLEL SAFE
AS $$
    SELECT p_left_id IS NOT NULL
        AND p_right_id IS NOT NULL
        AND p_left_id <> p_right_id
        AND EXISTS (
            SELECT 1
            FROM user_blocks b
            WHERE b.app_id = p_app_id
              AND (
                  (b.blocker_id = p_left_id AND b.blocked_id = p_right_id)
                  OR (b.blocker_id = p_right_id AND b.blocked_id = p_left_id)
              )
        );
$$;

CREATE FUNCTION social_user_muted(p_app_id UUID, p_viewer_id UUID, p_subject_id UUID)
RETURNS BOOLEAN
LANGUAGE sql
STABLE
PARALLEL SAFE
AS $$
    SELECT p_viewer_id IS NOT NULL
        AND p_subject_id IS NOT NULL
        AND p_viewer_id <> p_subject_id
        AND EXISTS (
            SELECT 1
            FROM user_mutes m
            WHERE m.app_id = p_app_id
              AND m.muter_id = p_viewer_id
              AND m.muted_id = p_subject_id
        );
$$;
