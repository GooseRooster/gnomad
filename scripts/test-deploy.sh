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
DEV_TAP="gnomad-dev/local"

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
  echo "Done. Installed at: $(command -v gnomad)"
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

# Homebrew (recent versions) refuses to install a bare formula file that
# isn't part of a tap, so we maintain a tiny local tap and drop the
# formula into it directly rather than re-tapping on every run.
DEV_TAP_DIR="$(brew --repository "$DEV_TAP")"
if [[ ! -d "$DEV_TAP_DIR" ]]; then
  echo "==> Creating local dev tap ($DEV_TAP)"
  SCAFFOLD="$(mktemp -d)"
  mkdir -p "$SCAFFOLD/Formula"
  git -C "$SCAFFOLD" init -q
  git -C "$SCAFFOLD" add -A
  git -C "$SCAFFOLD" -c user.name="gnomad-dev" -c user.email="dev@localhost" commit -q -m "init" --allow-empty
  brew tap "$DEV_TAP" "file://$SCAFFOLD"
  rm -rf "$SCAFFOLD"
fi

FORMULA="$DEV_TAP_DIR/Formula/gnomad.rb"
mkdir -p "$(dirname "$FORMULA")"

cat > "$FORMULA" <<EOF
class Gnomad < Formula
  desc "A lightweight TUI for managing tinted color schemes in the GNOME shell (local dev build)"
  homepage "https://github.com/GooseRooster/gnomad"
  license "GPL-3.0-or-later"
  url "file://$REPO_ROOT", using: :git, branch: "$BRANCH"
  version "$LOCAL_VERSION"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--locked", "--root", prefix, "--path", "."
  end

  test do
    system "#{bin}/gnomad", "--help"
  end
end
EOF

echo "==> Installing gnomad from local checkout (branch: $BRANCH, commit: $COMMIT)"
brew install --build-from-source "$DEV_TAP/gnomad"

echo
echo "Done. Installed at: $(command -v gnomad)"
echo "This is a local dev build (branch: $BRANCH, commit: $COMMIT)."
echo "Run '$(basename "$0") --revert' to go back to the released version."
