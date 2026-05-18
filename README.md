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
- Lightweight Python agent (<50MB)
- Automatic model download from S3
- Atomic model switching (no downtime)
- Checksum verification
- Health monitoring
- Offline operation support

---

## Architecture

```
OTAedge Platform (cloud/on-prem)
├── Rust backend (axum)
├── PostgreSQL database
├── MinIO/S3 storage
├── Redis cache
└── Next.js dashboard

Edge Agent (on device)
├── WebSocket connection
├── Model download manager
├── Model switcher
├── Health monitor
└── Inference runner
```

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
- Python 3.10+ (for agent development)
- Node.js 18+ (for frontend development)

### Installation

1. **Clone repository**
```bash
git clone https://github.com/yourusername/OTAedge
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

---

## Installing Edge Agent

On each edge device (Raspberry Pi, Jetson, etc.):

```bash
# Download installation script
curl -O https://your-server.com/agent/install.sh

# Run installation
sudo bash install.sh --server http://your-server.com --token <registration_token>

# Or manual installation
git clone https://github.com/yourusername/OTAedge
cd OTAedge/agent
pip install -r requirements.txt
sudo cp install/otaedge.service /etc/systemd/system/
sudo systemctl enable otaedge
sudo systemctl start otaedge
```

The agent will:
1. Register with the platform
2. Send heartbeat every 30 seconds
3. Listen for deployment commands
4. Download and switch models as instructed

---

## Documentation

- **[Validation Plan](plans/03-validation-customer-discovery.md)** - How to validate this problem before building
- **[Technical Implementation Plan](plans/01-technical-mvp-implementation.md)** - 12-week build roadmap
- **[Go-to-Market Strategy](plans/02-go-to-market-strategy.md)** - How to acquire first 50 customers

See [docs/](docs/) for additional documentation.

---

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Setup

```bash
# Backend
cd backend
python -m venv venv
source venv/bin/activate
pip install -r requirements.txt
uvicorn app.main:app --reload

# Frontend
cd frontend
npm install
npm run dev

# Agent
cd agent
python -m venv venv
source venv/bin/activate
pip install -r requirements.txt
python main.py
```

---

## Roadmap

**MVP (12 weeks)**:
- Raspberry Pi + TensorFlow Lite support
- Basic deployment (all-at-once)
- Manual rollback
- Simple metrics

**v1.0 (Month 4-6)**:
- Phased rollouts (10% → 100%)
- ONNX support
- A/B testing
- Advanced metrics and alerts

**v2.0 (Month 7-12)**:
- Rust agent (smaller, faster)
- Enterprise features (SSO, audit logs)
- Compliance certifications (SOC2, HIPAA)
- Marketplace for pre-trained models

---

## License

MIT License - see [LICENSE](LICENSE) for details.

---

## Support

- **Issues**: [GitHub Issues](https://github.com/yourusername/OTAedge/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/OTAedge/discussions)
- **Email**: support@otaedge.com
- **Slack**: [Join our community](https://otaedge.com/slack)

---

## Acknowledgments

Built with:
- [FastAPI](https://fastapi.tiangolo.com/)
- [Next.js](https://nextjs.org/)
- [PostgreSQL](https://www.postgresql.org/)
- [TensorFlow Lite](https://www.tensorflow.org/lite)

---

## Star History

If this project helps you, please give us a ⭐!

[![Star History Chart](https://api.star-history.com/svg?repos=yourusername/OTAedge&type=Date)](https://star-history.com/#yourusername/OTAedge&Date)

---

**OTAedge** - Making edge AI deployments reliable, observable, and effortless.
