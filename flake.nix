{
  description = "Kitsune2 packages";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-24.11";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs@{ flake-parts, crane, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" "aarch64-darwin" ];

      perSystem = { self', inputs', pkgs, system, ... }:
        let
          inherit (pkgs) lib;

          craneLib = crane.mkLib pkgs;

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
          packages = {
            inherit bootstrap-srv;
          };
        };
    };
}
