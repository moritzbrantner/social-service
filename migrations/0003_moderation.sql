CREATE TYPE moderation_target_type AS ENUM ('profile', 'post', 'comment', 'media', 'conversation', 'message');
CREATE TYPE moderation_content_state AS ENUM ('active', 'hidden', 'removed');
CREATE TYPE moderation_account_state AS ENUM ('active', 'suspended', 'banned');
CREATE TYPE moderation_case_state AS ENUM ('open', 'investigating', 'resolved', 'dismissed');
CREATE TYPE moderation_role AS ENUM ('moderator', 'admin');
CREATE TYPE moderation_restriction_scope AS ENUM ('profile', 'media', 'post', 'comment', 'follow', 'chat');

CREATE TABLE moderation_role_bindings (
    app_id UUID NOT NULL,
    user_id UUID NOT NULL,
    role moderation_role NOT NULL,
    granted_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    PRIMARY KEY (app_id, user_id)
);

CREATE TABLE moderation_account_states (
    app_id UUID NOT NULL,
    user_id UUID NOT NULL,
    state moderation_account_state NOT NULL,
    reason TEXT CHECK (reason IS NULL OR char_length(reason) <= 2000),
    updated_by UUID NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    PRIMARY KEY (app_id, user_id)
);

CREATE TABLE moderation_restrictions (
    app_id UUID NOT NULL,
    user_id UUID NOT NULL,
    scope moderation_restriction_scope NOT NULL,
    reason TEXT CHECK (reason IS NULL OR char_length(reason) <= 2000),
    updated_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    PRIMARY KEY (app_id, user_id, scope)
);

CREATE TABLE moderation_content_states (
    app_id UUID NOT NULL,
    target_type moderation_target_type NOT NULL,
    target_id UUID NOT NULL,
    state moderation_content_state NOT NULL,
    reason TEXT CHECK (reason IS NULL OR char_length(reason) <= 2000),
    updated_by UUID NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    PRIMARY KEY (app_id, target_type, target_id)
);

CREATE TABLE moderation_cases (
    id UUID PRIMARY KEY,
    app_id UUID NOT NULL,
    target_type moderation_target_type NOT NULL,
    target_id UUID NOT NULL,
    state moderation_case_state NOT NULL DEFAULT 'open',
    opened_by UUID NOT NULL,
    resolution_note TEXT CHECK (resolution_note IS NULL OR char_length(resolution_note) <= 4000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version BIGINT NOT NULL DEFAULT 1,
    UNIQUE (app_id, id)
);

CREATE TABLE moderation_reports (
    id UUID PRIMARY KEY,
    app_id UUID NOT NULL,
    case_id UUID NOT NULL,
    reporter_id UUID NOT NULL,
    target_type moderation_target_type NOT NULL,
    target_id UUID NOT NULL,
    category TEXT NOT NULL CHECK (char_length(category) BETWEEN 1 AND 80),
    context TEXT CHECK (context IS NULL OR char_length(context) <= 2000),
    idempotency_key TEXT CHECK (idempotency_key IS NULL OR char_length(idempotency_key) BETWEEN 1 AND 128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (app_id, id),
    FOREIGN KEY (app_id, case_id) REFERENCES moderation_cases(app_id, id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX moderation_reports_idempotency_idx
    ON moderation_reports (app_id, reporter_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE TABLE moderation_audit_events (
    id UUID PRIMARY KEY,
    app_id UUID NOT NULL,
    actor_id UUID NOT NULL,
    action TEXT NOT NULL CHECK (char_length(action) BETWEEN 1 AND 128),
    target_kind TEXT NOT NULL CHECK (char_length(target_kind) BETWEEN 1 AND 64),
    target_id UUID,
    reason TEXT CHECK (reason IS NULL OR char_length(reason) <= 2000),
    previous_state TEXT CHECK (previous_state IS NULL OR char_length(previous_state) <= 255),
    new_state TEXT CHECK (new_state IS NULL OR char_length(new_state) <= 255),
    case_id UUID,
    correlation_id TEXT CHECK (correlation_id IS NULL OR char_length(correlation_id) <= 128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX moderation_cases_queue_idx
    ON moderation_cases (app_id, state, created_at ASC);
CREATE INDEX moderation_cases_target_idx
    ON moderation_cases (app_id, target_type, target_id, created_at DESC);
CREATE INDEX moderation_reports_case_idx
    ON moderation_reports (app_id, case_id, created_at ASC);
CREATE INDEX moderation_content_state_idx
    ON moderation_content_states (app_id, target_type, state, target_id);
CREATE INDEX moderation_account_state_idx
    ON moderation_account_states (app_id, state, user_id);
CREATE INDEX moderation_restrictions_user_idx
    ON moderation_restrictions (app_id, user_id, scope);
CREATE INDEX moderation_audit_idx
    ON moderation_audit_events (app_id, created_at DESC, id DESC);

CREATE FUNCTION reject_moderation_report_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'moderation reports are immutable';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER moderation_reports_immutable
    BEFORE UPDATE OR DELETE ON moderation_reports
    FOR EACH ROW EXECUTE FUNCTION reject_moderation_report_mutation();

CREATE FUNCTION reject_moderation_audit_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'moderation audit events are append-only';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER moderation_audit_append_only
    BEFORE UPDATE OR DELETE ON moderation_audit_events
    FOR EACH ROW EXECUTE FUNCTION reject_moderation_audit_mutation();
