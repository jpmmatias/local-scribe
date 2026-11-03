#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Explicit developer action only. The desktop never invokes this script.
# Keep development models separate from an application's live data directory.
node scripts/setup-models.mjs "${1:-base}" "${2:-.build/models}"
