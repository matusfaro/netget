#!/usr/bin/env bash
# Cargo wrapper script for isolated builds across multiple Claude instances
# Usage: ./cargo-isolated.sh <cargo-args>
# Example: ./cargo-isolated.sh build --release --all-features
#
# Changes:
# - Disables incremental so sccache can cache rustc outputs (debug defaults to incremental) [web:19][web:13].
# - Remaps absolute paths so per-session target dirs don't perturb cache keys [web:31][web:21].
# - Ensures RUSTC_WRAPPER=sccache is set and stable across sessions [web:5][web:24].
# - Keeps your --print-last behavior unchanged.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Print usage if no arguments
if [[ $# -eq 0 ]]; then
  echo "Usage: ./cargo-isolated.sh <cargo-args>" >&2
  echo "       ./cargo-isolated.sh --print-last" >&2
  echo "" >&2
  echo "Examples:" >&2
  echo "  ./cargo-isolated.sh build --no-default-features --features tcp" >&2
  echo "  ./cargo-isolated.sh test --features tcp | tail -50" >&2
  echo "  ./cargo-isolated.sh --print-last | grep 'error'" >&2
  echo "" >&2
  echo "Options:" >&2
  echo "  --print-last     Print the most recent log file for this session" >&2
  echo "  --skip-cleanup   Skip cleanup of old target directories" >&2
  exit 1
fi

# --print-last passthrough
if [[ "${1:-}" == "--print-last" ]]; then
  TMP_DIR="${SCRIPT_DIR}/tmp"
  if [[ ! -d "$TMP_DIR" ]]; then
    echo "Error: No tmp/ directory found. Run ./cargo-isolated.sh first." >&2
    exit 1
  fi
  SESSION_PID="${CARGO_SESSION_PID:-$PPID}"
  LOG_FILE=$(ls -t "${TMP_DIR}/netget-"*"-${SESSION_PID}.log" 2>/dev/null | head -n 1)
  if [[ -z "${LOG_FILE:-}" ]]; then
    echo "Error: No log files found for session PID ${SESSION_PID}" >&2
    echo "Run ./cargo-isolated.sh first to generate logs." >&2
    exit 1
  fi
  echo "Reading log: $LOG_FILE" >&2
  echo "============================" >&2
  cat "$LOG_FILE"
  exit 0
fi

# Session ID: keep isolation by parent shell PID unless provided
export CARGO_SESSION_PID="${CARGO_SESSION_PID:-$PPID}"

# Derive an isolated target directory but try to avoid it poisoning cache keys
ISOLATED_ROOT="${SCRIPT_DIR}/target-claude"
ISOLATED_DIR="${ISOLATED_ROOT}/claude-${CARGO_SESSION_PID}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ISOLATED_DIR}"

# Make sure sccache is active and consistent
export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"        # enables sccache for rustc calls [web:5][web:24]
export SCCACHE_DIR="${SCCACHE_DIR:-$HOME/.cache/sccache}" # stable cache location [web:24]
# Optional but recommended:
export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-20G}"

# Disable incremental across profiles to make sccache effective
# Env vars override cargo config and profile defaults [web:19][web:13]
export CARGO_INCREMENTAL=0
export CARGO_BUILD_INCREMENTAL=false

# Stabilize paths that enter the rustc commandline and debuginfo
# Use remap-path-prefix (stable) so differing CARGO_TARGET_DIR and workspace paths don't change cache keys [web:31][web:21]
# Map project root and the session-specific target root to stable anchors
PROJECT_ROOT="$(pwd)"
# Note: Use --remap-path-prefix (double dash) which is the correct flag for rustc
ADD_REMAPS="--remap-path-prefix=${PROJECT_ROOT}=/proj --remap-path-prefix=${ISOLATED_ROOT}=/tgt"
# Preserve user-provided flags while appending ours
export RUSTFLAGS="${RUSTFLAGS:-} ${ADD_REMAPS}"

# Optional: trim embedded paths consistently via Cargo’s trim-paths when available
# Users can also enable in .cargo/config.toml: [profile.release] trim-paths = true [web:21]

# Forward to your cargo driver
CARGO_USE_ISOLATION=true exec "${SCRIPT_DIR}/cargo.sh" "$@"
