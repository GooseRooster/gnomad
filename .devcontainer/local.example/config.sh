# Dev container -- personal config.
#
# Copy this whole directory up one level to .devcontainer/local/ (gitignored):
#
#     cp -r .devcontainer/local.example .devcontainer/local
#
# Edit the values below, then rebuild the container. local/setup.sh sources this.

# chezmoi dotfiles source repo to bootstrap inside the container.
DOTFILES_REPO="https://github.com/GooseRooster/dotfiles.git"

# Git identity for commits made inside the container (written to ~/.gitconfig;
# SSH auth is handled by the forwarded agent). Leave either empty to skip.
GIT_USER_NAME=""
GIT_USER_EMAIL=""
