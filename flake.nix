{
  description = "Kitsune2 packages";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-25.05";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs@{
      nixpkgs,
      flake-parts,
      crane,
      rust-overlay,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem =
        { pkgs, system, ... }:
        let
          rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

          craneLib = (crane.mkLib pkgs).overrideToolchain rust;

          bootstrap-srv = craneLib.buildPackage {
            pname = "bootstrap-srv";
            cargoExtraArgs = "-p kitsune2_bootstrap_srv";
            src = craneLib.cleanCargoSource ./.;
            nativeBuildInputs = [
              pkgs.perl
              pkgs.cmake
            ];
            buildInputs = [
              pkgs.openssl
            ];
            doCheck = false;
          };
        in
        {
          # Override the per system packages to include the rust overlay
          _module.args.pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          packages = {
            inherit bootstrap-srv;
          };

          devShells.default =
            with pkgs;
            mkShell {
              packages = [
                rustup
                cargo-make
                taplo
                perl
                cmake
                openssl
                llvmPackages.clang
                llvmPackages.libclang
              ];

              LIBCLANG_PATH = "${lib.getLib llvmPackages.libclang}/lib";

              shellHook = ''
                ${rustup}/bin/rustup toolchain install ${rust.version}
                ${rustup}/bin/rustup toolchain install nightly
              '';
            };
        };
    };
}
