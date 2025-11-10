#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ $# -eq 0 ]]; then
  echo "Usage: ./cargo-isolated.sh <cargo-args>" >&2
  echo "       ./cargo-isolated.sh --print-last" >&2
  exit 1
fi

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
    exit 1
  fi
  echo "Reading log: $LOG_FILE" >&2
  echo "============================" >&2
  cat "$LOG_FILE"
  exit 0
fi

export CARGO_SESSION_PID="${CARGO_SESSION_PID:-$PPID}"
ISOLATED_ROOT="${SCRIPT_DIR}/target-claude"
ISOLATED_DIR="${ISOLATED_ROOT}/claude-${CARGO_SESSION_PID}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ISOLATED_DIR}"

# Ensure sccache is used and consistent
export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"        # rustc wrapper hook [web:19][web:13]
export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-20G}"

# Disable incremental; env overrides profiles
export CARGO_INCREMENTAL=0                               # global off [web:19][web:26]

# Stabilize paths in debuginfo and diagnostics
PROJECT_ROOT="$(pwd)"
ADD_REMAPS=(
  "--remap-path-prefix=${PROJECT_ROOT}=/proj"
  "--remap-path-prefix=${ISOLATED_ROOT}=/tgt"
)
# Use CARGO_ENCODED_RUSTFLAGS to avoid word-splitting issues
ENCODED="${CARGO_ENCODED_RUSTFLAGS:-}"
for ((i=0; i<${#ADD_REMAPS[@]}; i++)); do
  if [[ -n "$ENCODED" ]]; then ENCODED+=$'\x1f'; fi
  ENCODED+="${ADD_REMAPS[$i]}"
done
export CARGO_ENCODED_RUSTFLAGS="$ENCODED"               # recommended for multi-flag injection [web:13][web:19]

# Optional: strip debuginfo in dev to further stabilize keys
# export CARGO_PROFILE_DEV_DEBUG=0                       # fewer path embeddings [web:19]

# Verify cargo.sh preserves env; do not exec rustc directly in cargo.sh
CARGO_USE_ISOLATION=true exec "${SCRIPT_DIR}/cargo.sh" "$@"
