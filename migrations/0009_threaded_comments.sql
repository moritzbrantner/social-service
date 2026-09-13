ALTER TABLE comments
    ADD COLUMN parent_comment_id UUID,
    ADD COLUMN deleted_at TIMESTAMPTZ;

ALTER TABLE comments DROP CONSTRAINT comments_body_check;

ALTER TABLE comments
    ADD CONSTRAINT comments_body_or_tombstone_check CHECK (
        (deleted_at IS NULL AND char_length(body) BETWEEN 1 AND 5000)
        OR (deleted_at IS NOT NULL AND body = '')
    ),
    ADD CONSTRAINT comments_app_post_id_unique UNIQUE (app_id, post_id, id),
    ADD CONSTRAINT comments_parent_same_post_fkey
        FOREIGN KEY (app_id, post_id, parent_comment_id)
        REFERENCES comments(app_id, post_id, id)
        ON DELETE RESTRICT;

CREATE INDEX comments_parent_created_idx
    ON comments (app_id, post_id, parent_comment_id, created_at ASC, id ASC);
