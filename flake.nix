{
  description = "edgedb-cli flake";
  inputs = {
    # <frameworks>
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";

    # <devtools>
    # for this project could be replaced with simplier `@numtide/devshell` project 
    devenv.url = "github:cachix/devenv";
    devenv.inputs.nixpkgs.follows = "nixpkgs";
    nix2container.url = "github:nlewo/nix2container"; # https://github.com/cachix/devenv/issues/528
    nix2container.inputs.nixpkgs.follows = "nixpkgs";

    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";

    flake-root.url = "github:srid/flake-root";

    # <app builders>
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
    fenix.inputs.rust-analyzer-src.follows = "";

    crane.url = "github:ipetkov/crane";
    crane.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = inputs:
    with builtins; let
      lib = inputs.nixpkgs.lib;
      createPkgs = system:
        import inputs.nixpkgs {
          inherit system;
          config.allowUnfree = true;
          overlays = with inputs; [
            (final: pkgs: {
              edgedb-server = let
                # https://packages.edgedb.com/archive/
                # Replace with input flake after Nixifing edgedb-server
                platform =
                  {
                    x86_64-linux = {
                      url = "https://packages.edgedb.com/archive/x86_64-unknown-linux-gnu.testing/edgedb-server-4.0-rc.1%2Bfd5ed53.tar.zst";
                      hash = "sha256-B4HuFTLLSYLmSKyDxerNgM13+SBu75BTlMysCi3yrMA=";
                    };
                    x86_64-darwin = {
                      url = "https://packages.edgedb.com/archive/x86_64-apple-darwin.testing/edgedb-server-4.0-rc.1%2B0dfbf64.tar.zst"; # TODO
                      hash = lib.fakeHash;
                    };
                  }
                  .${system};
              in
                pkgs.stdenvNoCC.mkDerivation {
                  name = "edgedb-server";
                  buildInputs = with pkgs; [python3];
                  nativeBuildInputs = with pkgs; [zstd] ++ lib.optionals (!pkgs.stdenv.isDarwin) [autoPatchelfHook];
                  dontPatchELF = pkgs.stdenv.isDarwin;
                  dontFixup = pkgs.stdenv.isDarwin;
                  src = pkgs.fetchurl {
                    url = platform.url;
                    sha256 = platform.hash;
                  };
                  installPhase = ''
                    mkdir $out
                    cp -r ./* $out
                  '';
                };
            })
          ];
        };
    in
      with lib;
        inputs.flake-parts.lib.mkFlake {inherit inputs;}
        {
          imports = with inputs; [
            flake-root.flakeModule
            treefmt-nix.flakeModule
            devenv.flakeModule
            ./shell.nix
          ];
          systems = ["x86_64-linux" "x86_64-darwin"];
          perSystem = {
            config,
            self',
            system,
            inputs',
            ...
          }: let
            pkgs = createPkgs system;
            rustToolchain = with inputs.fenix.packages.${system};
              combine [
                stable.rustc
                stable.cargo
              ];
          in {
            _module.args = {inherit lib pkgs rustToolchain;};
            treefmt = {
              projectRootFile = "flake.lock";
              flakeCheck = false;
              programs.alejandra.enable = true;
              programs.shellcheck.enable = true;
              programs.rustfmt.enable = true;
              programs.rustfmt.package = rustToolchain;
            };
            packages.edgedb-server = pkgs.edgedb-server;
            packages.edgedb-cli = let
              craneLib = inputs.crane.lib.${system}.overrideToolchain rustToolchain;
              pInfo = craneLib.crateNameFromCargoToml {cargoToml = ./Cargo.toml;};
              src = pkgs.lib.cleanSourceWith {
                src = craneLib.path ./.;
                filter = path: type: ((builtins.match ".*tests.*" path != null) || (craneLib.filterCargoSources path type));
              };
              common =
                pInfo
                // {
                  inherit src;
                  doCheck = false; # disable tests
                  nativeBuildInputs = [pkgs.pkg-config pkgs.openssl pkgs.openssl.dev];
                  buildInputs = [pkgs.openssl pkgs.openssl.dev] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [pkgs.libiconv];
                  OPENSSL_NO_VENDOR = "1";
                  preConfigure = ''export CARGO_MANIFEST_DIR="$(pwd)"'';
                };
              cargoArtifacts = craneLib.buildDepsOnly common;
            in
              craneLib.buildPackage (common // {inherit cargoArtifacts;});
            packages.default = self'.packages.edgedb-cli;
          };
        };
}
