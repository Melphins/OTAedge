-- Migration: Create alerts table for monitoring and alerting

CREATE TABLE IF NOT EXISTS alerts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    severity VARCHAR(20) NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
    title VARCHAR(255) NOT NULL,
    description TEXT NOT NULL,
    source VARCHAR(50) NOT NULL CHECK (source IN ('deployment', 'device', 'system', 'security')),
    status VARCHAR(20) NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'acknowledged', 'silenced', 'closed')),
    device_id UUID REFERENCES devices(id) ON DELETE SET NULL,
    deployment_id UUID REFERENCES deployments(id) ON DELETE SET NULL,
    acknowledged_at TIMESTAMPTZ,
    acknowledged_by UUID REFERENCES users(id) ON DELETE SET NULL,
    silenced_until TIMESTAMPTZ,
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for common queries
CREATE INDEX IF NOT EXISTS idx_alerts_status ON alerts(status);
CREATE INDEX IF NOT EXISTS idx_alerts_severity ON alerts(severity);
CREATE INDEX IF NOT EXISTS idx_alerts_source ON alerts(source);
CREATE INDEX IF NOT EXISTS idx_alerts_created_at ON alerts(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_alerts_device_id ON alerts(device_id);
CREATE INDEX IF NOT EXISTS idx_alerts_deployment_id ON alerts(deployment_id);
CREATE INDEX IF NOT EXISTS idx_alerts_device_user ON alerts(device_id, created_at DESC);

-- Comment
COMMENT ON TABLE alerts IS 'System alerts for deployment failures, device offline, and system errors';
COMMENT ON COLUMN alerts.severity IS 'Alert severity: info, warning, or critical';
COMMENT ON COLUMN alerts.source IS 'Alert source: deployment, device, system, or security';
COMMENT ON COLUMN alerts.status IS 'Alert status: open, acknowledged, silenced, or closed';