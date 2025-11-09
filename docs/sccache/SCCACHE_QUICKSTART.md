# sccache Quick Start Guide for NetGet + Claude Code for Web

This guide shows you how to set up remote compiler caching to speed up NetGet builds across Claude Code sessions.

## Why Use Remote sccache?

**Problem**: Building NetGet takes 1-2 minutes even with isolated targets
**Solution**: Remote cache persists across Claude Code sessions
**Result**: 40-50% faster builds after first compilation

## Installation (One-Time)

sccache is already installed in this session. To install in future sessions:

```bash
cargo install sccache
```

## Quick Start: Choose Your Backend

### Option A: Cloudflare R2 (Recommended)

**Best for**: Production use, large projects, persistence
**Free tier**: 10 GB storage, unlimited egress

1. **Create R2 bucket**:
   - Go to https://dash.cloudflare.com/
   - Navigate to R2 Object Storage → Create bucket
   - Name it: `netget-sccache` (or any name)
   - Note your Account ID (found on R2 overview page)

2. **Create API token**:
   - In R2, click "Manage R2 API Tokens"
   - Create token with "Object Read & Write" permissions
   - Save the Access Key ID and Secret Access Key

3. **Configure**:
   ```bash
   # Run the setup script
   bash /tmp/setup-sccache-r2.sh

   # Or manually set environment variables:
   export SCCACHE_BUCKET="netget-sccache"
   export SCCACHE_REGION="auto"
   export SCCACHE_ENDPOINT="https://<account-id>.r2.cloudflarestorage.com"
   export AWS_ACCESS_KEY_ID="<your-access-key>"
   export AWS_SECRET_ACCESS_KEY="<your-secret-key>"
   export RUSTC_WRAPPER=sccache
   ```

4. **Make persistent**:
   ```bash
   # Add to ~/.bashrc so it loads in every session
   echo 'source ~/.sccache-r2-env' >> ~/.bashrc
   ```

### Option B: Upstash Redis

**Best for**: Quick setup, smaller projects
**Free tier**: 1 GB storage, 200 GB bandwidth/month

1. **Create Redis database**:
   - Go to https://console.upstash.com/
   - Create database (select "Global" region)
   - Copy the TLS connection string (starts with `rediss://`)

2. **Configure**:
   ```bash
   # Run the setup script
   bash /tmp/setup-sccache-upstash.sh

   # Or manually:
   export SCCACHE_REDIS_ENDPOINT="rediss://default:<password>@<endpoint>.upstash.io:6379"
   export RUSTC_WRAPPER=sccache
   ```

3. **Make persistent**:
   ```bash
   echo 'source ~/.sccache-upstash-env' >> ~/.bashrc
   ```

### Option C: Local Cache (No Setup)

**Best for**: Testing, single session
**Free tier**: N/A (uses local disk)

```bash
export RUSTC_WRAPPER=sccache
# That's it! Uses ~/.cache/sccache by default
```

## Usage

### With NetGet's cargo-isolated.sh

**Option 1: Use wrapper script** (recommended)
```bash
# Copy the wrapper to NetGet directory
cp /tmp/cargo-sccache.sh /home/user/netget/

# Use instead of cargo-isolated.sh
./cargo-sccache.sh build --no-default-features --features tcp,http,dns
./cargo-sccache.sh test --features tcp
```

**Option 2: Modify cargo-isolated.sh** (permanent)
```bash
# Add these lines after the initial variable setup in cargo-isolated.sh:
if command -v sccache &> /dev/null; then
    export RUSTC_WRAPPER=sccache
fi
```

**Option 3: Set environment before running** (manual)
```bash
export RUSTC_WRAPPER=sccache
./cargo-isolated.sh build --no-default-features --features tcp
```

### Checking Cache Performance

```bash
# Show detailed statistics
sccache --show-stats

# Example output:
Compile requests                     47
Cache hits                           12
Cache misses                         12
Cache hits rate                   50.00 %
Cache location                  s3, name: netget-sccache
```

### Managing the Cache

```bash
# Zero statistics (for testing)
sccache --zero-stats

# Stop server (releases locks)
sccache --stop-server

# Start server (usually automatic)
sccache --start-server

# Check local cache size
du -sh ~/.cache/sccache/

# For R2/Redis: Check dashboard for remote cache size
```

## Expected Performance

Based on testing with a simple Rust project:

| Build Type | Without sccache | With sccache (cached) | Improvement |
|------------|-----------------|----------------------|-------------|
| Small project | 9.3s | 5.4s | 42% faster |
| NetGet (tcp,http,dns) | ~90s | ~50s | 44% faster* |
| NetGet (all features) | ~120s | ~70s | 42% faster* |

*Estimated based on proportional improvement

**Note**: Cache hit rate depends on:
- How many files you changed
- Whether dependencies were updated
- If Rust compiler version changed

## Troubleshooting

### "sccache not found"
```bash
cargo install sccache
```

### "Cache hits always 0"
```bash
# Check RUSTC_WRAPPER is set
echo $RUSTC_WRAPPER  # Should output: sccache

# Check server is running
sccache --show-stats  # Should show statistics, not error
```

### "Connection refused" (R2/Redis)
```bash
# Check environment variables are set
env | grep SCCACHE

# For R2, test endpoint:
curl -I $SCCACHE_ENDPOINT

# For Redis, check connection string format:
# Correct: rediss://default:pass@host:6379 (note: double 's' for TLS)
# Wrong: redis://... (single 's', no TLS)
```

### "Permission denied" (R2)
```bash
# Verify API token has "Object Read & Write" permissions
# Recreate token if needed with correct permissions
```

### Cache not persisting across sessions
```bash
# Make sure configuration is in ~/.bashrc or equivalent
cat ~/.bashrc | grep sccache

# If not, add:
echo 'source ~/.sccache-r2-env' >> ~/.bashrc  # For R2
# or
echo 'source ~/.sccache-upstash-env' >> ~/.bashrc  # For Upstash
```

## Integration with NetGet Development Workflow

### Scenario 1: Protocol Development
```bash
# First build (populate cache)
source ~/.sccache-r2-env
./cargo-sccache.sh build --no-default-features --features tcp

# Make changes to src/server/tcp/mod.rs
# Second build (cache hits on unchanged dependencies)
./cargo-sccache.sh build --no-default-features --features tcp
# ~40% faster!
```

### Scenario 2: Cross-Session Development
```bash
# Session 1 (Day 1)
./cargo-sccache.sh build --no-default-features --features tcp,http,dns
# Cache populated to R2

# Session 2 (Day 2 - new Claude Code instance)
# Environment loads ~/.sccache-r2-env automatically
./cargo-sccache.sh build --no-default-features --features tcp,http,dns
# Cache hits from Day 1! Faster build.
```

### Scenario 3: Testing Multiple Protocols
```bash
# Build with sccache for each protocol
for proto in tcp http dns redis ssh; do
    echo "Building $proto..."
    ./cargo-sccache.sh build --no-default-features --features $proto
    # Each build benefits from shared dependency cache
done
```

## Cost Estimate (Cloudflare R2)

Based on NetGet's typical usage:

| Metric | Usage | Free Tier | Status |
|--------|-------|-----------|--------|
| Storage | ~100-500 MB | 10 GB | ✅ Well within |
| Writes | ~1000/day | 1M/month | ✅ Well within |
| Reads | ~2000/day | 10M/month | ✅ Well within |
| Bandwidth | N/A | Unlimited | ✅ Always free |

**Estimated monthly cost**: $0.00 (free tier sufficient)

## Next Steps

1. ✅ Choose a backend (R2 recommended)
2. ✅ Run setup script (`setup-sccache-r2.sh` or `setup-sccache-upstash.sh`)
3. ✅ Test with a build: `./cargo-sccache.sh build --no-default-features --features tcp`
4. ✅ Check statistics: `sccache --show-stats`
5. ✅ Add to ~/.bashrc for persistence
6. ✅ Enjoy faster builds!

## Resources

- Full exploration report: `/tmp/SCCACHE_REMOTE_EXPLORATION.md`
- Setup scripts: `/tmp/setup-sccache-*.sh`
- Wrapper script: `/tmp/cargo-sccache.sh`
- sccache docs: https://github.com/mozilla/sccache

## Summary

**Time to setup**: 10-15 minutes
**Time saved per build**: 30-60 seconds (after first build)
**Persistence**: Across all Claude Code sessions
**Cost**: Free (with recommended providers)

🚀 **Start caching and speed up your NetGet development!**
