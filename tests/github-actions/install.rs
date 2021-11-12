use std::fs;
use std::io::Write;

use assert_cmd::{Command, assert::Assert};
use async_std::prelude::FutureExt;
use tokio::sync::oneshot;
use warp::Filter;
use warp::filters::path::path;

use crate::certs::Certs;

const UNIX_INST: &str =
    "curl --proto '=https' --tlsv1.2 -sSf https://localhost:8443 | sh -s -- -y";

trait OutputExt {
    fn context(self, name: &'static str, description: &'static str) -> Self;
}

impl OutputExt for Assert {
    fn context(mut self, name: &'static str, description: &'static str) -> Self
    {
        self = self.append_context(name, description);
        let out = self.get_output();
        println!("------ {}: {} (STDOUT) -----", name, description);
        println!("{}", String::from_utf8_lossy(&out.stdout));
        println!("------ {}: {} (STDERR) -----", name, description);
        println!("{}", String::from_utf8_lossy(&out.stderr));
        self
    }
}

#[test]
fn github_action_install() -> anyhow::Result<()> {
    let tokio = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let certs = Certs::new()?;
    let (shut_tx, shut_rx) = oneshot::channel();

    let plat = if cfg!(all(target_os="linux", target_arch="x86_64")) {
        "linux-x86_64"
    } else if cfg!(all(target_os="macos", target_arch="x86_64")) {
        "macos-x86_64"
    } else if cfg!(all(target_os="windows", target_arch="x86_64")) {
        "windows-x86_64"
    } else {
        panic!("unsupported platform");
    };

    let routes = warp::filters::path::end()
            .and(warp::fs::file("./edgedb-init.sh"))
        .or(path("dist")
            .and(path(plat))
            .and(path("edgedb-cli_latest"))
            .and(warp::filters::path::end())
            .and(warp::fs::file(env!("CARGO_BIN_EXE_edgedb"))));

    let server = warp::serve(routes)
        .tls()
        .cert(certs.nginx_cert)
        .key(certs.nginx_key)
        .run(([127, 0, 0, 1], 8443))
        .race(async move { shut_rx.await.ok(); });

    let http = tokio.spawn(server);
    std::thread::sleep(std::time::Duration::new(10, 0));

    if cfg!(windows) {
        fs::copy(env!("CARGO_BIN_EXE_edgedb"), "edgedb-init.exe")?;
        Command::new("edgedb-init.exe")
            .arg("-y")
            .assert()
            .context("edgedb-init", "self install by command name")
            .success();
    } else {
        let mut tmpfile = tempfile::NamedTempFile::new()?;
        tmpfile.write_all(&certs.ca_cert)?;
        Command::new("sh")
            .arg("-c")
            .arg("-e")
            .arg(UNIX_INST)
            .env("CURL_CA_BUNDLE", tmpfile.path())
            .env("EDGEDB_PKG_ROOT", "https://localhost:8443")
            .assert()
            .context("curl shebang", "command-line install")
            .success()
            .stdout(predicates::str::contains(
                "The EdgeDB command-line tool is now installed"));
    }

    shut_tx.send(()).ok();
    tokio.block_on(http)?;

    let bin_dir = dirs::executable_dir()
        .unwrap_or(dirs::data_dir().unwrap().join("edgedb").join("bin"));
    let edgedb = if cfg!(windows) {
        bin_dir.join("edgedb.exe")
    } else {
        bin_dir.join("edgedb")
    };

    Command::new(&edgedb)
        .arg("--version")
        .assert()
        .context("version", "command-line version option")
        .success()
        .stdout(predicates::str::contains(
            concat!("EdgeDB CLI ", env!("CARGO_PKG_VERSION"))));

    if !cfg!(windows) {
        Command::new(&edgedb)
            .arg("server").arg("install")
            .assert()
            .context("install", "server installation")
            .success();

        // TODO(tailhook) check output somehow
        Command::new(&edgedb)
            .arg("server").arg("list-versions")
            .assert()
            .context("list-versions", "list versions of the server")
            .success();

        // Extra install is no-op
        Command::new(&edgedb)
            .arg("server").arg("install")
            .assert()
            .context("install-2", "check that installation conficts")
            .success();

        // TODO(tailhook) update to old version
        Command::new(&edgedb)
            .arg("server").arg("install").arg("--version=1.0-rc.2")
            .assert()
            .context("install-old", "older version of edgedb")
            .success();

        Command::new(&edgedb)
            .arg("server").arg("list-versions")
            .arg("--installed-only").arg("--column=major-version")
            .assert()
            .context("installed only", "check the version is installed")
            .success()
            .stdout(predicates::str::contains("1.0-rc.2"));

        if cfg!(target_os="macos") {
            Command::new(&edgedb)
                .arg("instance").arg("create").arg("inst1")
                .assert()
                .context("create-1", "created `inst1`")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("logs").arg("inst1")
                .assert()
                .context("log-1-0", "logs of `inst1`")
                .success();

            Command::new(&edgedb)
                .arg("--admin").arg("--instance").arg("inst1")
                .arg("--wait-until-available=20s")
                .arg("query").arg("SELECT 1")
                .assert()
                .context("query-1", "query `inst1` first time")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("status").arg("inst1")
                .assert()
                .context("status-1", "status `inst1` first time")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("restart").arg("inst1")
                .assert()
                .context("restart-1", "restart `inst1`")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("logs").arg("inst1")
                .assert()
                .context("log-1-1", "logs of `inst1`")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("status").arg("inst1")
                .assert()
                .context("status-1-1", "status `inst1` after restart")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("stop").arg("inst1")
                .env("RUST_LOG", "warn,edgedb::process=debug")
                .assert()
                .context("stop-1", "stop `inst1`")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("status").arg("inst1")
                .assert()
                .context("status-1-2", "status `inst1` after stop")
                .code(3);

            Command::new(&edgedb)
                .arg("instance").arg("create").arg("second")
                    .arg("--version=1.0-rc.2")
                .assert()
                .context("create-2", "create `second`")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("start").arg("second")
                .assert()
                .context("start-2", "start `second`")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("start").arg("inst1")
                .assert()
                .context("start-1-3", "start `inst1` again")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("status").arg("second")
                .assert()
                .context("status-2", "status `second`")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("logs").arg("inst1")
                .assert()
                .context("log-1-2", "logs of `inst1`")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("logs").arg("second")
                .assert()
                .context("log-2", "logs of `second`")
                .success();

            Command::new(&edgedb)
                .arg("instance").arg("status").arg("inst1")
                .assert()
                .context("status-1-4", "status of `inst1`")
                .success();

            /*
            Command::new(&edgedb)
                .arg("instance").arg("upgrade").arg("inst1")
                .arg("--to-latest").arg("--force")
                .assert()
                .context("upgrade-1", "force upgrade `inst1` to latest")
                .success();

            Command::new(&edgedb)
                .arg("--admin").arg("--instance").arg("inst1")
                .arg("--wait-until-available=20s")
                .arg("query").arg("SELECT 1")
                .assert()
                .context("query-1-2", "query `inst1` after upgrade")
                .success();
            */

            Command::new(&edgedb)
                .arg("instance").arg("destroy").arg("second")
                .assert()
                .context("destroy-2", "destroy `second` instance")
                .success();

        }

        if cfg!(target_os="macos") {
            Command::new(&edgedb)
                .arg("--admin").arg("--instance").arg("inst1")
                .arg("--wait-until-available=20s")
                .arg("query").arg("SELECT 1")
                .assert()
                .context("query-1a", "late query of `inst1`")
                .success();
        }
    }

    Ok(())
}
