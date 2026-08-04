#!/usr/bin/env bash
# Optional per-developer personalization hook, chained after setup-repo.sh.
# Runs .devcontainer/local/setup.sh if the developer created one -- the whole
# .devcontainer/local/ directory is gitignored, so teammates and CI get the pure
# base and this is a silent no-op. Copy .devcontainer/local.example to get started.
set -euo pipefail

LOCAL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/local"
if [ -f "$LOCAL_DIR/setup.sh" ]; then
  echo "==> Personal setup: running $LOCAL_DIR/setup.sh"
  bash "$LOCAL_DIR/setup.sh"
else
  echo "==> No .devcontainer/local/setup.sh -- skipping personal setup (base only)."
fi
