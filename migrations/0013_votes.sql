CREATE TYPE social_vote_target_type AS ENUM ('post', 'comment');
CREATE TYPE social_vote_value AS ENUM ('up', 'down');

CREATE TABLE votes (
    app_id UUID NOT NULL,
    target_type social_vote_target_type NOT NULL,
    target_id UUID NOT NULL,
    post_id UUID,
    comment_id UUID,
    user_id UUID NOT NULL,
    vote_value social_vote_value NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (app_id, target_type, target_id, user_id),
    CHECK (
        (target_type = 'post' AND post_id IS NOT NULL AND post_id = target_id AND comment_id IS NULL)
        OR
        (target_type = 'comment' AND comment_id IS NOT NULL AND comment_id = target_id AND post_id IS NULL)
    ),
    FOREIGN KEY (app_id, post_id) REFERENCES posts(app_id, id) ON DELETE CASCADE,
    FOREIGN KEY (app_id, comment_id) REFERENCES comments(app_id, id) ON DELETE CASCADE,
    FOREIGN KEY (app_id, user_id) REFERENCES profiles(app_id, user_id) ON DELETE CASCADE
);

CREATE INDEX votes_target_aggregate_idx
    ON votes (app_id, target_type, target_id, vote_value);
