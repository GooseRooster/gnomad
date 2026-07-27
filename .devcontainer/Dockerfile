# Dev environment for gnomad: the Rust toolchain plus the three runtime
# binaries gnomad shells out to (git, gowall, tinty), so `cargo build`/`cargo
# run` work out of the box without a GNOME session backing them.
#
# Based on the standard Rust devcontainer image for its preinstalled,
# already PATH-wired cargo/rustup.
FROM mcr.microsoft.com/devcontainers/rust:1

ARG GOWALL_VERSION=v0.2.4

USER vscode

# tinty: published on crates.io, installs cleanly with the base image's cargo.
RUN cargo install --locked tinty

# gowall: no published crate/apt package; grab the prebuilt Linux amd64
# release tarball rather than pulling in a whole Go toolchain for one binary.
RUN curl -fsSL "https://github.com/Achno/gowall/releases/download/${GOWALL_VERSION}/gowall-amd64-linux.tar.gz" \
        -o /tmp/gowall.tar.gz \
    && sudo tar -xzf /tmp/gowall.tar.gz -C /usr/local/bin gowall \
    && rm /tmp/gowall.tar.gz

# git ships with the base image already.
