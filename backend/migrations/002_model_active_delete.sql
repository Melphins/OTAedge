ALTER TABLE models
ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT FALSE,
ADD COLUMN release_channel VARCHAR(50) NOT NULL DEFAULT 'latest';

CREATE UNIQUE INDEX idx_models_active_channel
ON models (name, release_channel)
WHERE is_active;

CREATE INDEX idx_models_active_lookup
ON models (name, release_channel, version DESC)
WHERE is_active;
