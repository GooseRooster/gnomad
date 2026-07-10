#!/usr/bin/env bash
# Local test-deploy script for gnomad.
#
# Installs the current local checkout via Homebrew, building from source with
# cargo, so you can test changes as an actual installed binary without
# touching the published tap/formula.
#
# Homebrew builds from a git checkout of this repo (via a `url ..., using:
# :git` formula), so it only ever sees committed state — uncommitted changes
# are not picked up.
#
# Usage:
#   ./scripts/test-deploy.sh            # uninstall brew gnomad, reinstall from local repo
#   ./scripts/test-deploy.sh --revert   # uninstall local build, reinstall from GooseRooster/gnomad tap

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAP="GooseRooster/gnomad"

for cmd in brew git; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "error: '$cmd' not found in PATH" >&2
    exit 1
  fi
done

uninstall_if_present() {
  if brew list --formula 2>/dev/null | grep -qx gnomad; then
    echo "==> Uninstalling existing gnomad"
    brew uninstall --formula gnomad
  fi
}

if [[ "${1:-}" == "--revert" ]]; then
  uninstall_if_present
  echo "==> Ensuring tap $TAP is present"
  brew tap "$TAP" >/dev/null
  echo "==> Installing latest gnomad from $TAP"
  brew install "$TAP/gnomad"
  echo
  echo "Done. $(gnomad --version)"
  exit 0
fi

if [[ -n "${1:-}" ]]; then
  echo "error: unknown argument '$1' (expected --revert or no arguments)" >&2
  exit 1
fi

if [[ -n "$(git -C "$REPO_ROOT" status --porcelain)" ]]; then
  echo "error: you have uncommitted changes." >&2
  echo "Homebrew builds from a git checkout of the local repo, so it only sees committed state." >&2
  echo "Commit (or stash) your changes first, then re-run this script." >&2
  exit 1
fi

BRANCH="$(git -C "$REPO_ROOT" rev-parse --abbrev-ref HEAD)"
COMMIT="$(git -C "$REPO_ROOT" rev-parse --short HEAD)"
LOCAL_VERSION="$(grep -m1 '^version = ' "$REPO_ROOT/Cargo.toml" | sed -E 's/version = "(.*)"/\1/')-local"

uninstall_if_present

FORMULA_DIR="$(mktemp -d)"
trap 'rm -rf "$FORMULA_DIR"' EXIT
FORMULA="$FORMULA_DIR/gnomad.rb"

cat > "$FORMULA" <<EOF
class Gnomad < Formula
  desc "A lightweight TUI for managing tinted color schemes in the GNOME shell (local dev build)"
  homepage "https://github.com/GooseRooster/gnomad"
  license "GPL-3.0-or-later"
  url "file://$REPO_ROOT", using: :git, branch: "$BRANCH"
  version "$LOCAL_VERSION"

  def install
    system "cargo", "install", "--locked", "--root", prefix, "--path", "."
  end

  test do
    system "#{bin}/gnomad", "--version"
  end
end
EOF

echo "==> Installing gnomad from local checkout (branch: $BRANCH, commit: $COMMIT)"
brew install --build-from-source "$FORMULA"

echo
echo "Done. $(gnomad --version)"
echo "This is a local dev build — run '$(basename "$0") --revert' to go back to the released version."
