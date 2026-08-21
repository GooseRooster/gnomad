{ config, lib, pkgs, ... }:

let
  cfg = config.programs.gnomad;
in
{
  options.programs.gnomad = {
    enable = lib.mkEnableOption "gnomad, a TUI for managing GNOME base16/base24 colour schemes";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ../package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ../package.nix { }";
      description = "The gnomad package to install.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
