CREATE TABLE profiles (
    app_id UUID NOT NULL,
    user_id UUID NOT NULL,
    display_name TEXT NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 120),
    bio TEXT,
    avatar_media_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    PRIMARY KEY (app_id, user_id)
);

CREATE TABLE media_assets (
    id UUID PRIMARY KEY,
    app_id UUID NOT NULL,
    owner_id UUID NOT NULL,
    url TEXT NOT NULL CHECK (char_length(url) BETWEEN 1 AND 4096),
    content_type TEXT NOT NULL CHECK (char_length(content_type) BETWEEN 1 AND 255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    UNIQUE (app_id, id)
);

CREATE TABLE posts (
    id UUID PRIMARY KEY,
    app_id UUID NOT NULL,
    author_id UUID NOT NULL,
    body TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 10000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    UNIQUE (app_id, id)
);

CREATE TABLE post_media (
    app_id UUID NOT NULL,
    post_id UUID NOT NULL,
    media_id UUID NOT NULL,
    position SMALLINT NOT NULL CHECK (position >= 0),
    PRIMARY KEY (post_id, media_id),
    UNIQUE (post_id, position),
    FOREIGN KEY (app_id, post_id) REFERENCES posts(app_id, id) ON DELETE CASCADE,
    FOREIGN KEY (app_id, media_id) REFERENCES media_assets(app_id, id) ON DELETE RESTRICT
);

CREATE TABLE comments (
    id UUID PRIMARY KEY,
    app_id UUID NOT NULL,
    post_id UUID NOT NULL,
    author_id UUID NOT NULL,
    body TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 5000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    UNIQUE (app_id, id),
    FOREIGN KEY (app_id, post_id) REFERENCES posts(app_id, id) ON DELETE CASCADE
);

CREATE TABLE follows (
    app_id UUID NOT NULL,
    follower_id UUID NOT NULL,
    followed_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (app_id, follower_id, followed_id),
    CHECK (follower_id <> followed_id)
);

CREATE TABLE conversations (
    id UUID PRIMARY KEY,
    app_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    UNIQUE (app_id, id)
);

CREATE TABLE conversation_members (
    app_id UUID NOT NULL,
    conversation_id UUID NOT NULL,
    user_id UUID NOT NULL,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (conversation_id, user_id),
    FOREIGN KEY (app_id, conversation_id) REFERENCES conversations(app_id, id) ON DELETE CASCADE
);

CREATE TABLE messages (
    id UUID PRIMARY KEY,
    app_id UUID NOT NULL,
    conversation_id UUID NOT NULL,
    author_id UUID NOT NULL,
    body TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    UNIQUE (app_id, id),
    FOREIGN KEY (app_id, conversation_id) REFERENCES conversations(app_id, id) ON DELETE CASCADE,
    CHECK (body IS NULL OR char_length(body) BETWEEN 1 AND 10000)
);

CREATE TABLE message_media (
    app_id UUID NOT NULL,
    message_id UUID NOT NULL,
    media_id UUID NOT NULL,
    position SMALLINT NOT NULL CHECK (position >= 0),
    PRIMARY KEY (message_id, media_id),
    UNIQUE (message_id, position),
    FOREIGN KEY (app_id, message_id) REFERENCES messages(app_id, id) ON DELETE CASCADE,
    FOREIGN KEY (app_id, media_id) REFERENCES media_assets(app_id, id) ON DELETE RESTRICT
);

CREATE INDEX posts_app_created_idx ON posts (app_id, created_at DESC);
CREATE INDEX posts_app_author_created_idx ON posts (app_id, author_id, created_at DESC);
CREATE INDEX comments_post_created_idx ON comments (app_id, post_id, created_at ASC);
CREATE INDEX follows_followed_idx ON follows (app_id, followed_id);
CREATE INDEX conversation_members_user_idx ON conversation_members (app_id, user_id, joined_at DESC);
CREATE INDEX messages_conversation_created_idx ON messages (app_id, conversation_id, created_at DESC);
