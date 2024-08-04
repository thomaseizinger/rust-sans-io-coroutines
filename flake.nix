{
  inputs = {
    nixpkgs.url = "nixpkgs/nixos-23.11";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { nixpkgs, flake-utils, rust-overlay, ... } @ inputs:
    flake-utils.lib.eachDefaultSystem
      (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          libraries = with pkgs; [ ];
          packages = with pkgs; [ ];

          mkShellWithRustVersion = rustVersion: pkgs.mkShell {
            packages = [ ];
            buildInputs = rustVersion ++ packages;
            name = "rust-env";
            src = ".";

            LD_LIBRARY_PATH = "${pkgs.lib.makeLibraryPath libraries}:$LD_LIBRARY_PATH${pkgs.lib.makeLibraryPath libraries}:$LD_LIBRARY_PATH";
          };
        in
        {
          devShells.default = mkShellWithRustVersion [
            (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
          ];
        }
      );
}
