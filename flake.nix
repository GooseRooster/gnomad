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

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
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
