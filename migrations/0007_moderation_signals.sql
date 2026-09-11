CREATE TYPE moderation_signal_severity AS ENUM ('low', 'medium', 'high', 'critical');

CREATE TABLE moderation_signals (
    id UUID PRIMARY KEY,
    app_id UUID NOT NULL,
    case_id UUID,
    target_type moderation_target_type NOT NULL,
    target_id UUID NOT NULL,
    source TEXT NOT NULL CHECK (char_length(source) BETWEEN 1 AND 120),
    kind TEXT NOT NULL CHECK (char_length(kind) BETWEEN 1 AND 120),
    severity moderation_signal_severity NOT NULL,
    confidence DOUBLE PRECISION CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0)),
    model TEXT CHECK (model IS NULL OR char_length(model) BETWEEN 1 AND 200),
    model_version TEXT CHECK (model_version IS NULL OR char_length(model_version) BETWEEN 1 AND 120),
    evidence JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(evidence) = 'object' AND octet_length(evidence::text) <= 16384),
    idempotency_key TEXT NOT NULL CHECK (char_length(idempotency_key) BETWEEN 1 AND 128),
    observed_at TIMESTAMPTZ NOT NULL,
    ingested_by UUID NOT NULL,
    correlation_id TEXT CHECK (correlation_id IS NULL OR char_length(correlation_id) <= 128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (app_id, id),
    FOREIGN KEY (app_id, case_id) REFERENCES moderation_cases(app_id, id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX moderation_signals_idempotency_idx
    ON moderation_signals (app_id, source, idempotency_key);
CREATE INDEX moderation_signals_queue_idx
    ON moderation_signals (app_id, severity DESC, observed_at DESC, id DESC);
CREATE INDEX moderation_signals_target_idx
    ON moderation_signals (app_id, target_type, target_id, observed_at DESC, id DESC);
CREATE INDEX moderation_signals_case_idx
    ON moderation_signals (app_id, case_id, observed_at ASC, id ASC)
    WHERE case_id IS NOT NULL;

CREATE FUNCTION reject_moderation_signal_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'moderation signals are immutable';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER moderation_signals_immutable
    BEFORE UPDATE OR DELETE ON moderation_signals
    FOR EACH ROW EXECUTE FUNCTION reject_moderation_signal_mutation();
