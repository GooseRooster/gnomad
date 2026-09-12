{
  description = "gnomad — a lightweight TUI for managing tinted color schemes in the GNOME shell";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system: {
        default = self.packages.${system}.gnomad;
        gnomad = nixpkgs.legacyPackages.${system}.callPackage ./package.nix { };
      });

      nixosModules = {
        default = self.nixosModules.gnomad;
        gnomad = ./modules/gnomad.nix;
      };

      # Dev shell. Provides everything gnomad needs at build/run time
      # (rustc/cargo from nixpkgs — no host rustup required — plus the
      # runtime deps git/gowall/tinty/glib checked at startup).
      #
      #   * rust-analyzer — from nixpkgs, NOT nvim's mason. Mason's
      #     rust-analyzer is a prebuilt native binary that cannot run on
      #     NixOS; nvim's environment profile only ever expects this from
      #     PATH.
      #   * codelldb — the vscode-lldb standalone adapter exposing
      #     bin/codelldb on PATH (no top-level nixpkgs attr; the extension's
      #     passthru.adapter is the packaged standalone build). nvim's rust
      #     DAP locates it via PATH for the same mason reason.
      #   * ./.dev.local.sh sourced on shell entry if present — the
      #     per-developer personalization hook (gitignored; teammates
      #     without one see nothing).
      #
      # Entry points:
      #   * direnv users:  `direnv allow`   (auto-activates via .envrc)
      #   * everyone else: `nix develop`
      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.rust-analyzer
              pkgs.vscode-extensions.vadimcn.vscode-lldb.adapter
              pkgs.git
              pkgs.gowall
              pkgs.tinty
              pkgs.glib
              pkgs.bash
            ];

            shellHook = ''
              # ── Personal hook. Gitignored; teammates without one see nothing.
              #    Auto-created from .dev.local.sh.example if desired.
              if [ -f ./.dev.local.sh ]; then
                # shellcheck source=/dev/null
                . ./.dev.local.sh
              fi
            '';
          };
        });
    };
}
