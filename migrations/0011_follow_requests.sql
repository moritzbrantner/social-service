CREATE TABLE follow_requests (
    app_id UUID NOT NULL,
    requester_id UUID NOT NULL,
    target_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (app_id, requester_id, target_id),
    FOREIGN KEY (app_id, requester_id) REFERENCES profiles(app_id, user_id) ON DELETE CASCADE,
    FOREIGN KEY (app_id, target_id) REFERENCES profiles(app_id, user_id) ON DELETE CASCADE,
    CHECK (requester_id <> target_id)
);

CREATE INDEX follow_requests_target_created_idx
    ON follow_requests (app_id, target_id, created_at DESC, requester_id ASC);

CREATE TABLE follow_approvals (
    app_id UUID NOT NULL,
    requester_id UUID NOT NULL,
    target_id UUID NOT NULL,
    approved_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (app_id, requester_id, target_id),
    FOREIGN KEY (app_id, requester_id) REFERENCES profiles(app_id, user_id) ON DELETE CASCADE,
    FOREIGN KEY (app_id, target_id) REFERENCES profiles(app_id, user_id) ON DELETE CASCADE,
    FOREIGN KEY (app_id, requester_id, target_id)
        REFERENCES follows(app_id, follower_id, followed_id) ON DELETE CASCADE,
    CHECK (requester_id <> target_id)
);

CREATE INDEX follow_approvals_target_approved_idx
    ON follow_approvals (app_id, target_id, approved_at DESC, requester_id ASC);

CREATE FUNCTION social_cancel_pending_follow_request_after_unfollow()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
AS $$
BEGIN
    DELETE FROM follow_requests
    WHERE app_id = OLD.app_id
      AND requester_id = OLD.follower_id
      AND target_id = OLD.followed_id;
    RETURN OLD;
END;
$$;

CREATE TRIGGER follows_cancel_pending_request_after_delete
AFTER DELETE ON follows
FOR EACH ROW
EXECUTE FUNCTION social_cancel_pending_follow_request_after_unfollow();
