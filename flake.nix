{
  description = "The EdgeDB CLI";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs";
    flake-parts.url = "github:hercules-ci/flake-parts";

    # provides rust toolchain
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-analyzer-src.follows = "";
    };

    edgedb = {
      url = "github:edgedb/packages-nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-parts.follows = "flake-parts";
    };
  };

  outputs = inputs@{ flake-parts, fenix, edgedb, ... }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux" "x86_64-darwin"];
      perSystem = { config, system, pkgs, ... }:
        let
          fenix_pkgs = fenix.packages.${system};

          rustToolchain = (fenix_pkgs.complete.withComponents [
            "cargo"
            "clippy"
            "rust-src"
            "rustc"
            "rustfmt"
            "rust-analyzer"
          ]);

        in {            
          devShells.default = pkgs.mkShell {
            buildInputs = [
              rustToolchain

              # needed for running tests
              edgedb.packages.${system}.edgedb-server
            ];
          };
        };
    };
}
