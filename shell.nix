let
  sources = import ./npins;
  pkgs = import sources.nixpkgs {};
  rust-overlay = import sources.rust-overlay;

  pkgsWithOverlay = import sources.nixpkgs {
    overlays = [ (import sources.rust-overlay) ];
  };

  rustToolchain = pkgsWithOverlay.rust-bin.stable.latest.default.override {
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
