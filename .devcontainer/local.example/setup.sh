#!/usr/bin/env bash
# Dev container -- personal setup (runs after the baseline setup-repo.sh).
# Installs Homebrew (kept out of the shared image) and bootstraps your chezmoi
# dotfiles. Lives in the gitignored .devcontainer/local/, so it's entirely yours.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
[ -f "$here/config.sh" ] && source "$here/config.sh"
: "${DOTFILES_REPO:?set DOTFILES_REPO in .devcontainer/local/config.sh}"

# 1) Homebrew -- not baked into the shared image; personalization brings its own.
#    Ubuntu prerequisites first (https://docs.brew.sh/Homebrew-on-Linux); the
#    remote user has passwordless sudo via the common-utils feature.
if ! command -v brew >/dev/null 2>&1; then
  sudo apt-get update
  sudo apt-get install -y build-essential procps curl file git
  NONINTERACTIVE=1 /bin/bash -c \
    "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
fi
eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"

# 2) chezmoi dotfiles bootstrap. bootstrap-cli.sh --devcontainer records the
#    container flag and applies the dotfiles.
brew install chezmoi
chezmoi init "$DOTFILES_REPO"
( cd "$(chezmoi source-path)" && ./bootstrap-cli.sh --devcontainer )

# 3) Git identity, so commits from inside the container have an author.
#    Skipped if left empty in config.sh.
[ -n "${GIT_USER_NAME:-}" ]  && git config --global user.name  "$GIT_USER_NAME"
[ -n "${GIT_USER_EMAIL:-}" ] && git config --global user.email "$GIT_USER_EMAIL"
