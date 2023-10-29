{
  lib,
  inputs,
  ...
} @ rootArgs:
with builtins;
with lib; {
  perSystem = {
    system,
    self',
    pkgs,
    config,
    rustToolchain,
    ...
  } @ systemArgs:
    optionalAttrs (!lib.inPureEvalMode) {
      devenv.shells.default = let
        edb = rec {
          dir = "$DEVENV_STATE/edgedb";
          port = "10706";
          socket = "${dir}/.s.EDGEDB.admin.${port}";
          args = "--dsn=$EDGEDB_DSN --tls-security=insecure";
          # schemaArg = "--schema-dir=$DEVENV_ROOT/api/dbschema";
        };
        DEVENV_ROOT = systemArgs.config.devenv.shells.default.devenv.root;
        DEVENV_STATE = systemArgs.config.devenv.shells.default.devenv.state;
        DEVENV_PROFILE = systemArgs.config.devenv.shells.default.devenv.profile;
      in {
        dotenv.enable = true;
        env.RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

        enterShell = ''
          git update-index --assume-unchanged $DEVENV_ROOT/.env
          export CARGO_INSTALL_ROOT=$(${
            escapeShellArgs [
              "${pkgs.coreutils}/bin/realpath"
              "--no-symlinks"
              "${DEVENV_STATE}/cargo-install"
            ]
          })
          export PATH="$PATH:$CARGO_INSTALL_ROOT/bin"
          echo "$(tput setaf 63)🔳 Welcome in edgedb-cli environment $(tput sgr0)"
        '';

        packages =
          [
            pkgs.edgedb-server
            pkgs.watchexec

            rustToolchain
            pkgs.pkg-config
            pkgs.openssl
            pkgs.openssl.dev

            config.treefmt.build.wrapper
          ]
          ++ (attrValues config.treefmt.build.programs);

        scripts = {
          # shortcuts
          edgedb-dev.exec = "edgedb ${edb.args} $@";
          edgedb-dev-query.exec = ''echo "$@"; ${pkgs.edgedb}/bin/edgedb query --unix-path=${edb.socket} -u edgedb -d edgedb --connect-timeout=5s "$@"'';
          clean.exec = ''
            cd $DEVENV_ROOT
            rm -rf target
            rm -f result
          '';

          up.exec = let
            cmd = concatStringsSep " \\\n" [
              "${pkgs.edgedb-server}/bin/edgedb-server"
              "--data-dir=${edb.dir}"
              "--pidfile-dir=${edb.dir}"
              "--disable-dynamic-system-config"
              "--bind-address=127.0.0.1"
              "--port=${edb.port}"
              "--security=insecure_dev_mode"
              "--admin-ui=enabled"
            ];
          in ''
            ${cmd} &
              EDB_PID=$!
              edgedb-dev-query 'CREATE DATABASE edgedb-cli-test-db' ||:
              edgedb-dev-query 'ALTER ROLE edgedb SET password_hash := "SCRAM-SHA-256$4096:Sr2cQGdIgmXUThVpaJp1KA==$57A6JTPFlAVXAy3+8wp/BPJIAshlikUiReWrt9VcJRs=:PyHk43sVyWpHMiq8uyI1KWeNPzUy3cLrmd4WyzB3ccU="' ||:
              wait $EDB_PID
          '';
        };
      };
    };
}
