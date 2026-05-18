# OTAedge

**Deploy AI models to thousands of edge devices with one click.**

OTAedge is an open-source platform for managing AI model deployments to edge devices (Raspberry Pi, Jetson, etc.) with OTA updates, phased rollouts, instant rollback, and full observability.

---

## Quick Start

```bash
# Clone and setup
git clone https://github.com/yourusername/OTAedge
cd OTAedge

# Start all services
docker-compose up -d

# Initialize database
docker-compose exec backend sqlx migrate run

# Create admin user
curl -X POST http://localhost:8000/api/auth/register \
  -H "Content-Type: application/json" \
  -d '{"email":"admin@example.com","password":"admin123","org_name":"Demo Corp"}'

# Open dashboard
open http://localhost:3000
```

---

## Problem We Solve

Companies deploying AI to edge devices face critical challenges:
- **No visibility**: Don't know which devices have which model version
- **Risky updates**: One bad update can brick hundreds of devices
- **No rollback**: Manual recovery takes days
- **Testing gaps**: Can't test updates before full deployment
- **Cost**: Building custom deployment system costs $100K+

OTAedge solves this with:
- **Phased rollouts**: Deploy to 10% → 50% → 100% automatically
- **Instant rollback**: One-click revert to previous model
- **Real-time monitoring**: See deployment status, device health, model performance
- **Observability**: Track accuracy, latency, resource usage per device
- **Open source**: Self-hosted, no vendor lock-in

---

## Use Cases

- **IoT Startups**: Deploy computer vision models to 1000+ Raspberry Pi cameras
- **Automotive**: OTA updates for autonomous driving models on vehicle fleets
- **Industrial**: Predictive maintenance models on factory equipment
- **Robotics**: Navigation and perception model updates for warehouse robots
- **Drones**: Computer vision model updates for delivery drones

---

## Features

### For DevOps/ML Teams
- Web dashboard for managing all devices
- Model registry with versioning
- Phased deployment with canary testing
- One-click rollback
- Real-time metrics and alerts
- Device inventory and grouping
- Audit logs and compliance

### For Edge Devices
- Lightweight Rust agent (<5MB)
- Automatic model download from S3/MinIO
- Atomic model switching (no downtime)
- Checksum verification
- Health monitoring
- Offline operation support

---


## Supported Platforms

**Edge Devices**:
- Raspberry Pi 4/5 (Ubuntu/Debian)
- NVIDIA Jetson (Ubuntu)
- ARM64 Linux devices
- Support for more platforms in development

**Model Formats**:
- TensorFlow Lite (MVP)
- ONNX (coming soon)
- PyTorch (coming soon)

---

## Getting Started

### Prerequisites
- Docker & Docker Compose
- Rust 1.70+ (for backend development)
- Python 3.10+ (for agent scripting)
- Node.js 18+ (for frontend development)

### Installation

1. **Clone repository**
```bash
git clone https://github.com/Melphins/OTAedge.git
cd OTAedge
```

2. **Configure environment**
```bash
cp .env.example .env
# Edit .env with your settings
```

3. **Start services**
```bash
docker-compose up -d
```

4. **Initialize database**
```bash
docker-compose exec backend sqlx migrate run
```

5. **Create admin user**
```bash
curl -X POST http://localhost:8000/api/auth/register \
  -H "Content-Type: application/json" \
  -d '{"email":"admin@example.com","password":"admin123","org_name":"Demo Corp"}'
```

6. **Access dashboard**
   - Frontend: http://localhost:3000
   - API docs: http://localhost:8000/docs
   - MinIO console: http://localhost:9001 (minioadmin/minioadmin)
   - Prometheus: http://localhost:9090
   - Grafana: http://localhost:3002 (admin/admin)

---

## Backend Development

```bash
cd backend

# Build and run with watch
cargo watch -x run

# Or manually
cargo build --release
cargo run
```

### Backend Features
- RESTful API with OpenAPI/Swagger docs
- WebSocket support for real-time device communication
- Authentication with JWT
- RBAC for multi-tenant organizations
- File upload with multipart support
- Prometheus metrics middleware

---

## Frontend Development

```bash
cd frontend

# Install dependencies
npm install

# Run development server
npm run dev

# Build for production
npm run build
```

### Frontend Routes
- `/` - Landing page
- `/login` - User login
- `/register` - Organization signup
- `/dashboard` - Main dashboard with metrics
- `/devices` - Device inventory and management
- `/models` - Model registry
- `/deployments` - Deployment history and status
- `/alerts` - Alert configuration and history

---

## Edge Agent

The agent is written in Rust for minimal footprint and maximum reliability.

### Agent Development

```bash
cd agent

# Build and run
cargo run
```

### Installing Edge Agent

On each edge device (Raspberry Pi, Jetson, etc.):

```bash
# Download installation script
curl -O https://melphins.com/agent/install.sh  (later :) )

# Run installation
sudo bash install.sh --server http://melphins.com --token <registration_token>

# Or manual installation
git clone https://github.com/Melphins/OTAedge.git
cd OTAedge/agent
cargo build --release
sudo cp target/release/agent /usr/local/bin/
sudo cp install/otaedge.service /etc/systemd/system/
sudo systemctl enable otaedge
sudo systemctl start otaedge
```

The agent will:
1. Register with the platform
2. Send heartbeat every 30 seconds
3. Listen for deployment commands via WebSocket
4. Download and switch models as instructed

---

### Development Workflow

```bash
# Run tests
cargo test --workspace

# Run migrations
cargo sqlx migrate run

# Check formatting
cargo fmt --check

# Lint code
cargo clippy -- -D warnings
```

---

## License

MIT License - see [LICENSE](LICENSE) for details.

---

**OTAedge** - Making edge AI deployments reliable, observable, and effortless.