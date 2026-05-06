{
  description = "QuicHash - high-performance cryptographic hash utility";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        craneLib = crane.mkLib pkgs;

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        my-crate = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });
      in
      {
        packages.default = my-crate;

        apps.default = flake-utils.lib.mkApp {
          drv = my-crate;
          exePath = "/bin/hash";
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ my-crate ];

          packages = with pkgs; [
            rustc
            cargo
          ];
        };
      });
}
