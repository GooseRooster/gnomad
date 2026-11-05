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
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};

          # The optional ChromaLeon -> tinty bridge (assets/chromaleon-bridge).
          # Packaged so Nix users can install it without the shell installer:
          # the scripts end up in $out/bin with CHROMALEON_PALETTE /
          # CHROMALEON_PIXELS pointed at the copies beside them.
          chromaleonBridge = pkgs.runCommand "gnomad-chromaleon-bridge"
            { nativeBuildInputs = [ pkgs.makeWrapper ]; }
            ''
              mkdir -p $out/bin
              cp ${./assets/chromaleon-bridge}/chromaleon_tinty \
                 ${./assets/chromaleon-bridge}/chromaleon_tinty_watch \
                 $out/bin/
              cp ${./assets/chromaleon-bridge}/chromaleon_palette.py \
                 ${./assets/chromaleon-bridge}/chromaleon_pixels.js \
                 $out/bin/
              chmod +x $out/bin/*
              wrapProgram $out/bin/chromaleon_tinty \
                --set CHROMALEON_PALETTE $out/bin/chromaleon_palette.py \
                --set CHROMALEON_PIXELS $out/bin/chromaleon_pixels.js \
                --prefix PATH : ${pkgs.lib.makeBinPath [
                  pkgs.coreutils pkgs.dconf pkgs.gjs pkgs.imagemagick
                  pkgs.python3 pkgs.tinty pkgs.util-linux
                ]}
              wrapProgram $out/bin/chromaleon_tinty_watch \
                --prefix PATH : ${pkgs.lib.makeBinPath [
                  pkgs.coreutils pkgs.dconf pkgs.gjs pkgs.imagemagick
                  pkgs.python3 pkgs.tinty pkgs.util-linux
                ]}
            '';
        in
        {
          default = self.packages.${system}.gnomad;
          gnomad = nixpkgs.legacyPackages.${system}.callPackage ./package.nix { };
          chromaleon-bridge = chromaleonBridge;
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
      #   * per-developer personalization lives in ./.envrc.local
      #     (gitignored, sourced by .envrc) — nothing personal in this
      #     flake. Teammates without one see nothing.
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
          };
        });
    };
}
