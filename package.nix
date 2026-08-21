{
  lib,
  rustPlatform,
  makeWrapper,
  git,
  gowall,
  tinty,
  glib,
  bash,
}:

rustPlatform.buildRustPackage {
  pname = "gnomad";
  version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
      ./assets
    ];
  };

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [ makeWrapper ];

  # gnomad shells out to these at runtime (resolved from PATH). Wrapping them in
  # makes the package self-contained on NixOS regardless of the ambient PATH.
  # `gnome-extensions` is deliberately left out: it only matters inside a live
  # GNOME session, where it is already on PATH.
  postInstall = ''
    wrapProgram "$out/bin/gnomad" \
      --prefix PATH : ${lib.makeBinPath [ git gowall tinty glib bash ]}
  '';

  meta = with lib; {
    description = "A lightweight TUI for managing tinted color schemes in the GNOME shell";
    homepage = "https://github.com/GooseRooster/gnomad";
    license = licenses.gpl3Plus;
    mainProgram = "gnomad";
    platforms = platforms.linux;
  };
}
