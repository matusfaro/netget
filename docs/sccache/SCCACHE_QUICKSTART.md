# sccache Quickstart Guide

## What is sccache?

[sccache](https://github.com/mozilla/sccache) is a compiler cache developed by Mozilla. It caches compilation artifacts to speed up repeated builds. For Rust projects, this can dramatically reduce build times (from minutes to seconds for incremental changes).

## Why Use sccache with NetGet?

NetGet compiles 50+ network protocol implementations. With sccache:
- **First build**: Normal compile time (1-2min with `--all-features`)
- **Subsequent builds**: 10-30s for most changes (70-90% faster)
- **Works across**: Different feature flags, git branches, cargo-isolated sessions
- **Shared cache**: Multiple developers can share the same S3 bucket

## Local Disk Cache (Simplest)

### Setup

```bash
# Install sccache
brew install sccache  # macOS
# OR
cargo install sccache

# Configure (add to ~/.zshrc or ~/.bashrc)
export RUSTC_WRAPPER=sccache
export SCCACHE_DIR=~/.cache/sccache
export SCCACHE_CACHE_SIZE="10G"
```

### Verify

```bash
# Show current stats
sccache --show-stats

# Build NetGet
./cargo-isolated.sh build --no-default-features --features tcp

# Check cache hits
sccache --show-stats
# You should see "Compile requests: X, Cache hits: Y"
```

### Limitations

- Cache not shared across machines
- Cache cleared when disk space runs low
- No collaboration with team members

## AWS S3 Cache (Recommended for Teams)

### Prerequisites

- AWS account
- AWS CLI installed (`brew install awscli`)
- Basic understanding of AWS IAM

### Step 1: Create S3 Bucket

```bash
# Create bucket (use unique name, S3 bucket names are global)
aws s3 mb s3://netget-sccache-YOUR_ORG_NAME --region us-east-1

# Enable versioning (optional but recommended)
aws s3api put-bucket-versioning \
  --bucket netget-sccache-YOUR_ORG_NAME \
  --versioning-configuration Status=Enabled

# Set lifecycle policy to expire old objects (optional, saves costs)
cat > ./tmp/lifecycle.json <<'EOF'
{
  "Rules": [
    {
      "Id": "ExpireOldCacheObjects",
      "Status": "Enabled",
      "Expiration": {
        "Days": 90
      },
      "NoncurrentVersionExpiration": {
        "NoncurrentDays": 7
      }
    }
  ]
}
EOF

aws s3api put-bucket-lifecycle-configuration \
  --bucket netget-sccache-YOUR_ORG_NAME \
  --lifecycle-configuration file://./tmp/lifecycle.json
```

### Step 2: Create IAM User with Minimum Permissions

#### Create IAM Policy

Create a file `sccache-s3-policy.json`:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "SccacheS3ReadWrite",
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:PutObject",
        "s3:DeleteObject"
      ],
      "Resource": "arn:aws:s3:::netget-sccache-YOUR_ORG_NAME/*"
    },
    {
      "Sid": "SccacheS3ListBucket",
      "Effect": "Allow",
      "Action": [
        "s3:ListBucket"
      ],
      "Resource": "arn:aws:s3:::netget-sccache-YOUR_ORG_NAME"
    }
  ]
}
```

**Permission Breakdown**:
- `s3:GetObject` - Read cached compilation artifacts
- `s3:PutObject` - Write new compilation artifacts to cache
- `s3:DeleteObject` - Required for cache eviction (sccache manages cache size)
- `s3:ListBucket` - List objects in bucket (required for cache lookups)

**Security Notes**:
- Policy scoped to ONLY the sccache bucket (not all S3 buckets)
- No `s3:*` wildcard permissions
- No bucket deletion permissions (`s3:DeleteBucket` not granted)
- No permissions to modify bucket policies or ACLs
- Read-only in IAM Console (cannot escalate privileges)

#### Create IAM User and Attach Policy

```bash
# Create IAM policy
aws iam create-policy \
  --policy-name SccacheS3Access \
  --policy-document file://sccache-s3-policy.json

# Create IAM user
aws iam create-user --user-name sccache-netget

# Attach policy to user (replace ACCOUNT_ID with your AWS account ID)
aws iam attach-user-policy \
  --user-name sccache-netget \
  --policy-arn arn:aws:iam::ACCOUNT_ID:policy/SccacheS3Access

# Create access keys
aws iam create-access-key --user-name sccache-netget
```

**Save the output** - you'll get:
```json
{
  "AccessKey": {
    "AccessKeyId": "AKIA...",
    "SecretAccessKey": "wJalrXUtn..."
  }
}
```

**IMPORTANT**: Save both values securely. The secret key is only shown once.

### Step 3: Configure sccache

#### Option A: Environment Variables (Temporary)

```bash
# Add to current session
export RUSTC_WRAPPER=sccache
export SCCACHE_BUCKET=netget-sccache-YOUR_ORG_NAME
export SCCACHE_REGION=us-east-1
export AWS_ACCESS_KEY_ID=AKIA...
export AWS_SECRET_ACCESS_KEY=wJalrXUtn...
```

#### Option B: AWS Credentials File (Recommended)

```bash
# Configure AWS CLI (creates ~/.aws/credentials)
aws configure --profile sccache-netget
# AWS Access Key ID: AKIA...
# AWS Secret Access Key: wJalrXUtn...
# Default region name: us-east-1
# Default output format: json

# Add to ~/.zshrc or ~/.bashrc
export RUSTC_WRAPPER=sccache
export SCCACHE_BUCKET=netget-sccache-YOUR_ORG_NAME
export SCCACHE_REGION=us-east-1
export AWS_PROFILE=sccache-netget
```

#### Option C: IAM Role (EC2/Lambda/ECS only)

If running on AWS infrastructure, use IAM roles instead of access keys:

```bash
# Attach the SccacheS3Access policy to your EC2 instance role
export RUSTC_WRAPPER=sccache
export SCCACHE_BUCKET=netget-sccache-YOUR_ORG_NAME
export SCCACHE_REGION=us-east-1
# No AWS_ACCESS_KEY_ID or AWS_SECRET_ACCESS_KEY needed
```

### Step 4: Verify S3 Cache

```bash
# Clear local stats
sccache --stop-server

# Check configuration
sccache --show-stats
# Should show "Cache location: S3, bucket: netget-sccache-YOUR_ORG_NAME"

# Build with cold cache
./cargo-isolated.sh build --no-default-features --features tcp

# Check stats
sccache --show-stats
# First build: Cache misses: X, Cache hits: 0

# Rebuild (should use S3 cache)
./cargo-isolated.sh clean
./cargo-isolated.sh build --no-default-features --features tcp

# Check stats again
sccache --show-stats
# Second build: Cache hits: X (70-90% of compile requests)
```

### Step 5: Share with Team (Optional)

Share the following with team members:

1. **Bucket name**: `netget-sccache-YOUR_ORG_NAME`
2. **AWS Region**: `us-east-1`
3. **IAM Setup**: Create additional IAM users with same policy OR share read-only access:

**Read-Only Policy** (for team members who only consume cache):

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "SccacheS3ReadOnly",
      "Effect": "Allow",
      "Action": [
        "s3:GetObject"
      ],
      "Resource": "arn:aws:s3:::netget-sccache-YOUR_ORG_NAME/*"
    },
    {
      "Sid": "SccacheS3ListBucket",
      "Effect": "Allow",
      "Action": [
        "s3:ListBucket"
      ],
      "Resource": "arn:aws:s3:::netget-sccache-YOUR_ORG_NAME"
    }
  ]
}
```

## Troubleshooting

### Cache Not Working

```bash
# Check sccache is running
sccache --show-stats

# Check environment variables
env | grep SCCACHE
env | grep AWS

# Test S3 access manually
aws s3 ls s3://netget-sccache-YOUR_ORG_NAME --profile sccache-netget

# Enable debug logging
export SCCACHE_LOG=debug
sccache --stop-server
./cargo-isolated.sh build --no-default-features --features tcp
# Check ~/.cache/sccache/sccache.log or /tmp/sccache.log
```

### Permission Denied

```bash
# Verify IAM policy is attached
aws iam list-attached-user-policies --user-name sccache-netget

# Test S3 write permission
echo "test" > ./tmp/test.txt
aws s3 cp ./tmp/test.txt s3://netget-sccache-YOUR_ORG_NAME/test.txt --profile sccache-netget
aws s3 rm s3://netget-sccache-YOUR_ORG_NAME/test.txt --profile sccache-netget
```

### High S3 Costs

```bash
# Check bucket size
aws s3 ls s3://netget-sccache-YOUR_ORG_NAME --recursive --summarize --human-readable

# Set up lifecycle policy (see Step 1)
# Consider shorter expiration (30-60 days instead of 90)

# Or use CloudWatch to monitor costs:
# https://console.aws.amazon.com/cloudwatch/
```

## Cost Estimation

**S3 Storage** (us-east-1):
- $0.023 per GB/month (Standard tier)
- NetGet cache: ~5-10 GB (full build with all features)
- Cost: $0.12-$0.23/month

**S3 Requests**:
- PUT/COPY/POST/LIST: $0.005 per 1,000 requests
- GET/SELECT: $0.0004 per 1,000 requests
- Typical usage: 10,000 requests/month
- Cost: $0.05/month

**Total**: ~$0.20-$0.30/month (negligible for build time savings)

## Performance Comparison

| Scenario | No Cache | Local Cache | S3 Cache |
|----------|----------|-------------|----------|
| Clean build (--all-features) | 90-120s | 90-120s | 90-120s |
| Rebuild (no changes) | 90-120s | 5-10s | 10-15s |
| Rebuild (small change) | 60-90s | 10-20s | 15-25s |
| Different feature flags | 60-90s | 30-40s | 10-20s |
| Different git branch | 60-90s | 5-10s | 10-15s |

**S3 overhead**: +5s (network latency for cache uploads/downloads)

**Recommendation**: Use S3 cache for team collaboration, use local cache for personal development if network is slow.

## Advanced: Combining Local + S3 Cache

sccache supports multiple cache layers (local disk + S3):

```bash
# NOT YET SUPPORTED - Feature request: https://github.com/mozilla/sccache/issues/???
# sccache currently uses EITHER local OR S3, not both
# Workaround: Use local cache with manual S3 sync (complex, not recommended)
```

## References

- [sccache GitHub](https://github.com/mozilla/sccache)
- [sccache S3 Configuration](https://github.com/mozilla/sccache/blob/main/docs/S3.md)
- [AWS IAM Best Practices](https://docs.aws.amazon.com/IAM/latest/UserGuide/best-practices.html)
- [AWS S3 Pricing](https://aws.amazon.com/s3/pricing/)
