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
