#!/usr/bin/env bash
# Release script for gnomad.
# Run from the repo root: ./scripts/release.sh
# Requires: cargo, cargo-cross, gh (GitHub CLI), git, sha256sum

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAP_DIR="$HOME/Development/homebrew-gnomad"
CARGO_TOML="$REPO_ROOT/Cargo.toml"

# ── Preflight checks ──────────────────────────────────────────────────────────

for cmd in cargo gh git sha256sum; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "error: '$cmd' not found in PATH" >&2
    exit 1
  fi
done

if ! cargo cross --version &>/dev/null 2>&1; then
  echo "error: 'cargo-cross' not found — install with: cargo install cross" >&2
  exit 1
fi

if [[ ! -d "$TAP_DIR" ]]; then
  echo "error: tap directory not found: $TAP_DIR" >&2
  exit 1
fi

# ── Prompt for release details ────────────────────────────────────────────────

read -rp "New version (e.g. 0.4.0): " VERSION
if [[ -z "$VERSION" ]]; then
  echo "error: version cannot be empty" >&2
  exit 1
fi

read -rp "Release note (one-line summary): " RELEASE_NOTE
if [[ -z "$RELEASE_NOTE" ]]; then
  echo "error: release note cannot be empty" >&2
  exit 1
fi

echo
echo "Releasing gnomad v$VERSION: $RELEASE_NOTE"
echo

# ── Update Cargo.toml version ─────────────────────────────────────────────────

echo "==> Updating Cargo.toml to version $VERSION"
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" "$CARGO_TOML"

# ── Build x86_64 ─────────────────────────────────────────────────────────────

echo "==> Building x86_64 release binary"
cargo build --release --manifest-path "$CARGO_TOML"

X86_BINARY="$REPO_ROOT/target/release/gnomad"
X86_TARBALL="gnomad-x86_64-unknown-linux-gnu.tar.gz"

echo "==> Creating $X86_TARBALL"
(cd "$REPO_ROOT/target/release" && tar czf "$REPO_ROOT/$X86_TARBALL" gnomad)

# ── Build aarch64 ────────────────────────────────────────────────────────────

echo "==> Cross-compiling aarch64 release binary"
cargo cross build --release --target aarch64-unknown-linux-gnu --manifest-path "$CARGO_TOML"

ARM_TARBALL="gnomad-aarch64-unknown-linux-gnu.tar.gz"

echo "==> Creating $ARM_TARBALL"
(cd "$REPO_ROOT/target/aarch64-unknown-linux-gnu/release" && tar czf "$REPO_ROOT/$ARM_TARBALL" gnomad)

# ── Create GitHub release ─────────────────────────────────────────────────────

echo "==> Creating GitHub release v$VERSION"
gh release create "v$VERSION" \
  --repo GooseRooster/gnomad \
  --title "v$VERSION" \
  --notes "$RELEASE_NOTE" \
  "$REPO_ROOT/$X86_TARBALL" \
  "$REPO_ROOT/$ARM_TARBALL"

# ── Compute SHA256 hashes ────────────────────────────────────────────────────

echo "==> Computing SHA256 hashes"
X86_SHA=$(sha256sum "$REPO_ROOT/$X86_TARBALL" | awk '{print $1}')
ARM_SHA=$(sha256sum "$REPO_ROOT/$ARM_TARBALL" | awk '{print $1}')

echo "    x86_64:  $X86_SHA"
echo "    aarch64: $ARM_SHA"

# ── Update the brew formula ───────────────────────────────────────────────────

FORMULA="$TAP_DIR/Formula/gnomad.rb"
echo "==> Updating $FORMULA"

sed -i "s/version \".*\"/version \"$VERSION\"/" "$FORMULA"
sed -i "s|sha256 \"PLACEHOLDER_X86_64\"|sha256 \"$X86_SHA\"|" "$FORMULA"
sed -i "s|sha256 \"PLACEHOLDER_AARCH64\"|sha256 \"$ARM_SHA\"|" "$FORMULA"

# Handle the case where placeholders were already replaced in a previous release
# by targeting the architecture-specific lines directly.
DOWNLOAD_BASE="https://github.com/GooseRooster/gnomad/releases/download/v$VERSION"
sed -i \
  "s|releases/download/v[^/]*/gnomad-x86_64|releases/download/v$VERSION/gnomad-x86_64|" \
  "$FORMULA"
sed -i \
  "s|releases/download/v[^/]*/gnomad-aarch64|releases/download/v$VERSION/gnomad-aarch64|" \
  "$FORMULA"
sed -i "/gnomad-x86_64-unknown-linux-gnu.tar.gz\"/{n;s/sha256 \".*\"/sha256 \"$X86_SHA\"/}" \
  "$FORMULA"
sed -i "/gnomad-aarch64-unknown-linux-gnu.tar.gz\"/{n;s/sha256 \".*\"/sha256 \"$ARM_SHA\"/}" \
  "$FORMULA"

# ── Commit and push gnomad repo ───────────────────────────────────────────────

echo "==> Committing version bump in gnomad repo"
cd "$REPO_ROOT"
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to $VERSION"
git push

# ── Publish to crates.io ─────────────────────────────────────────────────────

echo "==> Publishing to crates.io"
cargo publish

# ── Commit and push tap repo ─────────────────────────────────────────────────

echo "==> Committing formula update in homebrew-gnomad tap"
cd "$TAP_DIR"
git add Formula/gnomad.rb
git commit -m "gnomad $VERSION"
git push

# ── Clean up tarballs ─────────────────────────────────────────────────────────

rm -f "$REPO_ROOT/$X86_TARBALL" "$REPO_ROOT/$ARM_TARBALL"

echo
echo "Done! gnomad v$VERSION released."
echo "Install with: brew install GooseRooster/gnomad/gnomad"
