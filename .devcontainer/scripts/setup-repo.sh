#!/usr/bin/env bash
# Baseline repo setup -- runs in every dev container (before the optional
# per-developer setup-local.sh).
set -euo pipefail

# Restore /tmp to sticky world-writable (1777). Some devcontainer Feature build
# steps leave /tmp as 0755 root-owned; under rootless-podman keep-id (non-root
# user) that makes /tmp unwritable and breaks any tool that creates temp dirs
# there. Features run after the Dockerfile, so fix it here. The user has
# passwordless sudo (common-utils).
sudo chmod 1777 /tmp

# devcontainers/features/common-utils:2 has a regression where it creates
# (or recreates) ~/.local and ~/.config as root-owned after install, which
# breaks anything that subsequently tries to mkdir under them -- notably
# chezmoi (`chezmoi init` fails with "permission denied" on ~/.local/share/chezmoi).
# The fix has to run here, in post-create: the Feature runs between the
# Dockerfile and us, so a Dockerfile chown would just be clobbered. Idempotent;
# ignores paths that don't exist yet.
sudo chown -R "$(id -u):$(id -g)" /home/vscode/.local /home/vscode/.config 2>/dev/null || true

# Toolchain (Rust, gowall, tinty, gsettings, gnome-extensions, dconf) is baked
# into the Dockerfile -- nothing to restore per-repo. `cargo build` is the
# baseline warmup so the first `cargo run` after a fresh container isn't a
# 30-second wait.
cargo build

# Sanity check the GNOME CLI tools are on PATH.
for cmd in gsettings gnome-extensions dconf tinty gowall; do
  command -v "$cmd" >/dev/null 2>&1 || {
    echo "==> $cmd missing from PATH -- devcontainer image is broken" >&2
    exit 1
  }
done

# Sanity check the DBus forwarding contract: a successful gsettings call
# means the session bus socket is mounted, DBUS_SESSION_BUS_ADDRESS resolves,
# and a real schema answered. The value capture uses the assignment-then-if
# pattern so the exit code is checked cleanly without a separate pipe.
if value=$(gsettings get org.gnome.desktop.interface color-scheme 2>/dev/null); then
  echo "==> DBus forwarding OK (host session bus reachable; color-scheme=${value})"
else
  echo "==> WARNING: gsettings could not read org.gnome.desktop.interface color-scheme." >&2
  echo "    The host session bus may not be reachable from the container." >&2
  echo "    Check that /run/user/1000/bus exists on the host and that DBUS_SESSION_BUS_ADDRESS" >&2
  echo "    is set in .devcontainer/devcontainer.json's containerEnv." >&2
fi
