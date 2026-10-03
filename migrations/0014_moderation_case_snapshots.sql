ALTER TABLE moderation_cases
    ADD COLUMN target_snapshot JSONB,
    ADD CONSTRAINT moderation_cases_target_snapshot_object
        CHECK (target_snapshot IS NULL OR jsonb_typeof(target_snapshot) = 'object');
