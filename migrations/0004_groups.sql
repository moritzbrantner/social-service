CREATE TYPE group_role AS ENUM ('owner', 'admin', 'member');

CREATE TABLE groups (
    id UUID PRIMARY KEY,
    app_id UUID NOT NULL,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    avatar_media_id UUID,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    UNIQUE (app_id, id),
    FOREIGN KEY (app_id, avatar_media_id) REFERENCES media_assets(app_id, id) ON DELETE RESTRICT
);

CREATE TABLE group_members (
    app_id UUID NOT NULL,
    group_id UUID NOT NULL,
    user_id UUID NOT NULL,
    role group_role NOT NULL DEFAULT 'member',
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    PRIMARY KEY (app_id, group_id, user_id),
    FOREIGN KEY (app_id, group_id) REFERENCES groups(app_id, id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX group_single_owner_idx
    ON group_members (app_id, group_id)
    WHERE role = 'owner';

CREATE TABLE group_membership_events (
    id UUID PRIMARY KEY,
    app_id UUID NOT NULL,
    group_id UUID NOT NULL,
    user_id UUID NOT NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('joined', 'left', 'role_changed')),
    actor_id UUID NOT NULL,
    previous_role group_role,
    new_role group_role,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX group_membership_events_group_idx
    ON group_membership_events (app_id, group_id, created_at ASC, id ASC);

CREATE FUNCTION reject_group_membership_event_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'group membership events are append-only';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER group_membership_events_append_only
    BEFORE UPDATE OR DELETE ON group_membership_events
    FOR EACH ROW EXECUTE FUNCTION reject_group_membership_event_mutation();

CREATE INDEX group_members_user_idx
    ON group_members (app_id, user_id, joined_at DESC, group_id);

CREATE TABLE group_conversations (
    app_id UUID NOT NULL,
    group_id UUID NOT NULL,
    conversation_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (app_id, group_id),
    UNIQUE (app_id, conversation_id),
    FOREIGN KEY (app_id, group_id) REFERENCES groups(app_id, id) ON DELETE CASCADE,
    FOREIGN KEY (app_id, conversation_id) REFERENCES conversations(app_id, id) ON DELETE RESTRICT
);
