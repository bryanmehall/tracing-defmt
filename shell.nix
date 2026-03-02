let
  sources = import ./npins;
  pkgs = import sources.nixpkgs {};
  rust-overlay = import sources.rust-overlay;

  pkgsWithOverlay = import sources.nixpkgs {
    overlays = [ (import sources.rust-overlay) ];
  };

  rustToolchain = pkgsWithOverlay.rust-bin.stable."1.93.0".default.override {
    extensions = [ "rust-src" ];
  };
in
pkgs.mkShell {
  buildInputs = [
    rustToolchain
    pkgs.pkg-config
    pkgs.udev
  ];

  shellHook = ''
    echo "Welcome to tracing-defmt dev shell"
  '';
}
