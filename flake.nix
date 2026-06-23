{
  description = "ArtifactX — Build Once. Package Once. Publish Everywhere.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "clippy" ];
          targets = [ "x86_64-unknown-linux-musl" ];
        };
        nativeBuildInputs = with pkgs; [ rust pkg-config ];
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "arx";
          version = "0.2.3";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = with pkgs; [ pkg-config ];
          meta = with pkgs.lib; {
            description = "Cross-platform package repository manager (apt + yum)";
            license = with licenses; [ gpl2Only mit asl20 ];
            mainProgram = "arx";
          };
        };

        devShells.default = pkgs.mkShell {
          inherit nativeBuildInputs;
          shellHook = ''
            echo "artifactx dev shell — arx $(cargo --version)"
          '';
        };
      });
}
