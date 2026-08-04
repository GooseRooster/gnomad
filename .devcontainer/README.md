# Dev container

Rust toolchain + the GNOME CLI tools gnomad shells out to (`gsettings`,
`gnome-extensions`, `dconf`), plus `tinty` (cargo) and `gowall` (release tarball)
-- so `cargo build`/`cargo run` work out of the box. No local Rust or GNOME
toolchain needed.

This container **forwards the host session DBus and the three CSS write paths**
into the container, so `cargo run -- --apply <slug>` from inside mutates the
live host GNOME session -- no nested GNOME required.

## What's baked in
- Non-root `vscode` user under rootless-podman `--userns=keep-id` (workspace stays writable)
- `/tmp` restored to `1777` in post-create (Features can clobber it → breaks tools)
- Homebrew on `PATH` for every `exec` (brew is installed by the personal hook, not a Feature)
- **SSH agent forwarding** -- private keys never enter the container; GitHub's
  host key is pre-seeded so pushing just works
- **GNOME session bus forwarding** -- `/run/user/1000/bus` from the host is
  bind-mounted in, and `DBUS_SESSION_BUS_ADDRESS` + `XDG_RUNTIME_DIR` are set,
  so gsettings/gnome-extensions inside the container talk to the live host session
- **CSS write-path bind mounts** -- host `~/.config/gtk-3.0`, `~/.config/gtk-4.0`,
  and `~/.local/share/themes` are bind-mounted at the same container paths, so
  CSS files gnomad writes land on the host. Other parts of `~/.config` and
  `~/.local/share` stay container-local (so the schemes-repo clone, wallpaper
  cache, and `gnomad.log` don't pollute the host)
- Gitignored `local/` personalization hook (dotfiles/editor), never committed
- Nested `.gitignore` + `.gitattributes` so the container config is self-contained and LF-safe

## Host prerequisites
- A running GNOME session on the host (Wayland or X11). `echo
  $DBUS_SESSION_BUS_ADDRESS` must show a unix socket under `/run/user/<uid>/`.
- The host user must have uid 1000 (matches the container's `vscode` user under
  `--userns=keep-id`). On a different uid, edits the `mounts` in
  `devcontainer.json` accordingly.
- **SSH agent** running with your git key loaded, launched from a shell where
  `SSH_AUTH_SOCK` is set (`ssh-add -l` to check).
- For the CSS bind mounts: `~/.config/gtk-3.0`, `~/.config/gtk-4.0`, and
  `~/.local/share/themes` will be created on the host if they don't already
  exist.

## Sanity check the forwarding

After the container starts (`scripts/setup-repo.sh` runs this automatically):

```bash
# Should print the host's current scheme (e.g. 'prefer-dark'):
gsettings get org.gnome.desktop.interface color-scheme

# Should list your enabled extensions:
gnome-extensions list --enabled
```

If both work, `cargo run -- --apply <slug>` from inside the container will
mutate the live host session.

## Personalization (optional)
Opt-in and never committed -- see [`local.example/README.md`](local.example/README.md).

## Files
- `devcontainer.json` -- the environment definition (plumbing + DBus/XDG mounts)
- `Dockerfile` -- Rust + gowall + tinty + gsettings/gnome-extensions/dconf
- `scripts/setup-repo.sh` -- baseline setup (`/tmp` fix, `cargo build`,
  gsettings/gnome-extensions/dconf PATH check, DBus reachability sanity check)
- `scripts/setup-local.sh` -- runs your gitignored `local/setup.sh` if present
- `local.example/` -- template for personal setup
- `local/` -- gitignored, your own personal setup (copied from local.example)
- `.gitignore` / `.gitattributes` -- nested, keep the container config self-contained + LF-safe
