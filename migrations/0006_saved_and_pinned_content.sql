CREATE TABLE post_saves (
    app_id UUID NOT NULL,
    user_id UUID NOT NULL,
    post_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (app_id, user_id, post_id),
    FOREIGN KEY (app_id, post_id) REFERENCES posts(app_id, id) ON DELETE CASCADE
);

CREATE INDEX post_saves_user_idx
    ON post_saves (app_id, user_id, created_at DESC, post_id);

ALTER TABLE messages
    ADD CONSTRAINT messages_app_conversation_message_unique
    UNIQUE (app_id, conversation_id, id);

CREATE TABLE conversation_message_pins (
    app_id UUID NOT NULL,
    conversation_id UUID NOT NULL,
    message_id UUID NOT NULL,
    pinned_by UUID NOT NULL,
    pinned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (app_id, conversation_id, message_id),
    FOREIGN KEY (app_id, conversation_id) REFERENCES conversations(app_id, id) ON DELETE CASCADE,
    FOREIGN KEY (app_id, conversation_id, message_id)
        REFERENCES messages(app_id, conversation_id, id) ON DELETE CASCADE
);

CREATE INDEX conversation_message_pins_list_idx
    ON conversation_message_pins (app_id, conversation_id, pinned_at DESC, message_id);
