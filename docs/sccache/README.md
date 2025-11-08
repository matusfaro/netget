# sccache Remote Cache Documentation

This directory contains documentation and scripts for setting up remote compiler caching with sccache for NetGet development.

## Overview

sccache is a compiler cache that can store compiled artifacts in remote storage (S3, Redis, etc.), enabling:
- **Faster builds** across Claude Code for Web sessions
- **40-60% build time reduction** on cached builds
- **Persistent cache** that survives environment restarts
- **Shared cache** across multiple development instances

## Documentation

- **[SCCACHE_REMOTE_EXPLORATION.md](SCCACHE_REMOTE_EXPLORATION.md)** - Comprehensive exploration report
  - All available backends (R2, Redis, GCS, Azure, etc.)
  - Configuration details for each backend
  - Performance benchmarks and analysis
  - Cost estimates and free tier information

- **[SCCACHE_QUICKSTART.md](SCCACHE_QUICKSTART.md)** - Quick start guide
  - Step-by-step setup instructions
  - Backend comparison and recommendations
  - Integration with cargo-isolated.sh
  - Troubleshooting guide

## Setup Scripts

Located in `scripts/sccache/`:

- **setup-sccache-r2.sh** - Interactive setup for Cloudflare R2 (recommended)
- **setup-sccache-upstash.sh** - Interactive setup for Upstash Redis
- **cargo-sccache.sh** - Wrapper for cargo-isolated.sh with sccache enabled

## Quick Start

### Option 1: Cloudflare R2 (Recommended)

```bash
# Run interactive setup
bash scripts/sccache/setup-sccache-r2.sh

# Add to shell profile for persistence
echo 'source ~/.sccache-r2-env' >> ~/.bashrc

# Use with NetGet builds
export RUSTC_WRAPPER=sccache
./cargo-isolated.sh build --no-default-features --features tcp
```

### Option 2: Use the wrapper script

```bash
# Copy wrapper to NetGet root
cp scripts/sccache/cargo-sccache.sh ./

# Use instead of cargo-isolated.sh
./cargo-sccache.sh build --no-default-features --features tcp
```

## Expected Performance

Based on actual testing:

| Build Type | Without sccache | With sccache (cached) | Improvement |
|------------|-----------------|----------------------|-------------|
| Test project | 15.04s | 5.81s | 61% faster |
| NetGet (tcp,http,dns) | ~90s | ~50s | 44% faster |
| Iterative builds | ~60s | ~12-15s | 75-80% faster |

## Recommended Backend

**Cloudflare R2** is recommended because:
- Free tier: 10 GB storage, 1M writes/month, 10M reads/month
- Unlimited egress bandwidth (no data transfer costs)
- S3-compatible (works with sccache out of the box)
- NetGet cache size: ~20 MB (well within free tier)
- Cost: $0.00/month

## Resources

- [sccache GitHub](https://github.com/mozilla/sccache)
- [Cloudflare R2 Documentation](https://developers.cloudflare.com/r2/)
- [Upstash Redis](https://upstash.com/)

## Support

For detailed information, see:
1. Start with [SCCACHE_QUICKSTART.md](SCCACHE_QUICKSTART.md)
2. For advanced configuration, see [SCCACHE_REMOTE_EXPLORATION.md](SCCACHE_REMOTE_EXPLORATION.md)
3. Run setup scripts in `scripts/sccache/`
