# Personal dev container setup (optional)

This dev container works out of the box with no personal setup. This directory is
a template for layering your own environment (dotfiles, editor, tooling) on top --
it never affects contributors or CI.

## Use it

    cp -r .devcontainer/local.example .devcontainer/local
    # edit .devcontainer/local/config.sh (DOTFILES_REPO + your git name/email)
    # rebuild the container

`.devcontainer/local/` is gitignored (see `.devcontainer/.gitignore`).
`scripts/setup-local.sh` runs `local/setup.sh` at the end of container creation if
it exists; otherwise nothing happens.

## What the template does
- installs Homebrew (kept out of the shared image),
- bootstraps your chezmoi dotfiles (`DOTFILES_REPO`) via `bootstrap-cli.sh --devcontainer`,
- sets your git identity (`GIT_USER_NAME` / `GIT_USER_EMAIL`) -- SSH auth itself is
  the forwarded agent, so no keys are needed here.

Edit `local/setup.sh` freely -- it's yours.
