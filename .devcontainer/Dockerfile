# Dev environment for gnomad: the Rust toolchain, the GNOME CLI tools gnomad
# shells out to (gsettings, gnome-extensions, dconf), and the two runtime
# binaries (tinty, gowall) -- so `cargo build`/`cargo run` work out of the box
# and talk to the host's GNOME session over a forwarded DBus socket.
#
# Based on the standard Rust devcontainer image for its preinstalled,
# already PATH-wired cargo/rustup.
FROM mcr.microsoft.com/devcontainers/rust:1

ARG GOWALL_VERSION=v0.2.4

USER root

# GNOME CLI tooling gnomad shells out to (see AGENTS.md "Runtime Dependencies"):
#   - libglib2.0-bin         -> /usr/bin/gsettings
#   - dconf-cli              -> /usr/bin/dconf
#   - gnome-shell-extensions -> /usr/bin/gnome-extensions
# gnome-session-bin pulls in the session helpers some of the above expect on
# Debian; harmless if nothing actually uses it.
# openssh-client is for ssh + ssh-keyscan (see SSH agent forwarding below).
RUN apt-get update && apt-get install -y --no-install-recommends \
  openssh-client \
  libglib2.0-bin \
  dconf-cli \
  gnome-shell-extensions \
  gnome-session-bin \
  && rm -rf /var/lib/apt/lists/*

# Baseline SSH host key so agent-forwarded git verifies github.com with no
# per-developer known_hosts file. '|| true' so a transient network failure
# doesn't fail the build.
RUN ssh-keyscan -t rsa,ecdsa,ed25519 github.com >> /etc/ssh/ssh_known_hosts 2>/dev/null || true

USER vscode

# tinty: published on crates.io, installs cleanly with the base image's cargo.
RUN cargo install --locked tinty

# gowall: no published crate/apt package; grab the prebuilt Linux amd64
# release tarball rather than pulling in a whole Go toolchain for one binary.
# common-utils sets up a rootless user, so we need sudo here
# hadolint ignore=DL3004
RUN curl -fsSL "https://github.com/Achno/gowall/releases/download/${GOWALL_VERSION}/gowall-amd64-linux.tar.gz" \
  -o /tmp/gowall.tar.gz \
  && tar -xzf /tmp/gowall.tar.gz -C /tmp gowall \
  && sudo cp /tmp/gowall /usr/local/bin/ \
  && sudo chmod +x /usr/local/bin/gowall \
  && rm /tmp/gowall.tar.gz /tmp/gowall

