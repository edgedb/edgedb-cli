{
  description = "The EdgeDB CLI";
  inputs = {
    # <frameworks>
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";

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
        };
    in
      with lib;
        inputs.flake-parts.lib.mkFlake {inherit inputs;}
        {
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
            packages.edgedb-cli = let
              craneLib = inputs.crane.lib.${system}.overrideToolchain rustToolchain;
              pInfo = craneLib.crateNameFromCargoToml {cargoToml = ./Cargo.toml;};
              src = pkgs.lib.cleanSourceWith {
                src = craneLib.path ./.;
                filter = path: type: ((builtins.match ".*tests.*" path != null) || (craneLib.filterCargoSources path type));
              };
              common = with pkgs;
                pInfo
                // {
                  inherit src;
                  doCheck = false; # disable tests
                  nativeBuildInputs = [pkg-config openssl openssl.dev];
                  buildInputs = [openssl openssl.dev] ++ lib.optionals stdenv.isDarwin [pkgs.libiconv];
                  OPENSSL_NO_VENDOR = "1";
                };
              cargoArtifacts = craneLib.buildDepsOnly common;
            in
              craneLib.buildPackage (common // {inherit cargoArtifacts;});
            packages.default = self'.packages.edgedb-cli;
          };
        };
}
