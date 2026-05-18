-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Users
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    email VARCHAR(255) UNIQUE NOT NULL,
    username VARCHAR(100) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    role VARCHAR(50) DEFAULT 'user',
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Models (tạo trước devices vì devices tham chiếu models)
CREATE TABLE models (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL,
    version INTEGER DEFAULT 1,
    file_name VARCHAR(500) NOT NULL,
    file_size_bytes BIGINT,
    s3_key VARCHAR(1000) NOT NULL,
    hash_sha256 CHAR(64) NOT NULL,
    model_format VARCHAR(100) DEFAULT 'unknown',
    metadata JSONB DEFAULT '{}',
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(name, version)
);

-- Devices (tham chiếu models)
CREATE TABLE devices (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    device_id VARCHAR(255) UNIQUE NOT NULL,
    name VARCHAR(255) NOT NULL,
    device_type VARCHAR(100),
    token VARCHAR(255) NOT NULL,
    status VARCHAR(50) DEFAULT 'offline',
    last_seen TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    current_model_id UUID REFERENCES models(id),
    model_version VARCHAR(50)
);

-- Refresh tokens
CREATE TABLE refresh_tokens (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(255) UNIQUE NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Deployments (tham chiếu devices và models)
CREATE TABLE deployments (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    device_id UUID REFERENCES devices(id),
    model_id UUID REFERENCES models(id),
    status VARCHAR(50) DEFAULT 'pending',
    rollout_strategy VARCHAR(50) DEFAULT 'all_at_once',
    rollout_percentage INTEGER DEFAULT 100,
    deployed_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    current_phase INTEGER DEFAULT 0,
    devices_target INTEGER,
    devices_deployed INTEGER,
    devices_succeeded INTEGER,
    devices_failed INTEGER,
    rollout_config JSONB DEFAULT '{}',
    rollback_of UUID REFERENCES deployments(id)
);

-- Deployment devices (junction table)
CREATE TABLE deployment_devices (
    deployment_id UUID REFERENCES deployments(id) ON DELETE CASCADE,
    device_id UUID REFERENCES devices(id) ON DELETE CASCADE,
    status VARCHAR(50) DEFAULT 'pending',
    previous_model_id UUID REFERENCES models(id),
    current_model_id UUID REFERENCES models(id),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    error_message TEXT,
    phase INTEGER DEFAULT 0,
    PRIMARY KEY (deployment_id, device_id)
);

-- Audit logs
CREATE TABLE audit_logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    actor_type VARCHAR(50) NOT NULL,
    actor_id UUID,
    action VARCHAR(100) NOT NULL,
    resource_type VARCHAR(50),
    resource_id UUID,
    old_state JSONB,
    new_state JSONB,
    ip_address VARCHAR(45),
    user_agent TEXT,
    metadata JSONB DEFAULT '{}',
    details JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Create indexes
CREATE INDEX idx_devices_token ON devices(token);
CREATE INDEX idx_devices_user_id ON devices(user_id);
CREATE INDEX idx_devices_current_model_id ON devices(current_model_id);
CREATE INDEX idx_models_name_version ON models(name, version DESC);
CREATE INDEX idx_deployments_device_status ON deployments(device_id, status);
CREATE INDEX idx_deployments_created_at ON deployments(created_at DESC);
CREATE INDEX idx_deployment_devices_deployment ON deployment_devices(deployment_id);
CREATE INDEX idx_deployment_devices_device ON deployment_devices(device_id);
CREATE INDEX idx_deployment_devices_status ON deployment_devices(status);
CREATE INDEX idx_refresh_tokens_user_id ON refresh_tokens(user_id);
CREATE INDEX idx_refresh_tokens_token_hash ON refresh_tokens(token_hash);
CREATE INDEX idx_audit_logs_actor ON audit_logs(actor_type, actor_id);
CREATE INDEX idx_audit_logs_resource ON audit_logs(resource_type, resource_id);
CREATE INDEX idx_audit_logs_created_at ON audit_logs(created_at DESC);
