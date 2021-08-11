use std::fs;
use std::io::Write;

use assert_cmd::Command;
use async_std::prelude::FutureExt;
use dirs::home_dir;
use predicates::boolean::PredicateBooleanExt;
use tokio::sync::oneshot;
use warp::Filter;
use warp::filters::path::path;

use crate::certs::Certs;

const UNIX_INST: &str =
    "curl --proto '=https' --tlsv1.2 -sSf https://localhost:8443 | sh -s -- -y";

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
        .success()
        .stdout(predicates::str::contains(
            concat!("EdgeDB CLI ", env!("CARGO_PKG_VERSION"))));

    if !cfg!(windows) {
        println!("Install");
        Command::new(&edgedb)
            .arg("server").arg("install")
            .assert()
            .success();

        // TODO(tailhook) check output somehow
        println!("List versions");
        Command::new(&edgedb)
            .arg("server").arg("list-versions")
            .assert()
            .success();

        // Extra install fails with code 51
        println!("Conflict on installing again");
        Command::new(&edgedb)
            .arg("server").arg("install")
            .assert()
            .code(51);

        println!("Install different version");
        Command::new(&edgedb)
            .arg("server").arg("install").arg("--version=1-beta2")
            .assert()
            .success();

        Command::new(&edgedb)
            .arg("server").arg("list-versions")
            .arg("--installed-only").arg("--column=major-version")
            .assert()
            .success()
            .stdout(predicates::str::contains("1-beta2"));

        if cfg!(target_os="macos") {
            println!("Init first");
            Command::new(&edgedb)
                .arg("server").arg("init").arg("inst1")
                .assert()
                .success();

            println!("Execute query");
            Command::new(&edgedb)
                .arg("--admin").arg("--instance").arg("inst1")
                .arg("--wait-until-available=20s")
                .arg("query").arg("SELECT 1")
                .assert()
                .success();

            println!("Status");
            Command::new(&edgedb)
                .arg("server").arg("status").arg("inst1")
                .assert()
                .success();

            println!("Restart");
            Command::new(&edgedb)
                .arg("server").arg("restart").arg("inst1")
                .assert()
                .success();

            println!("Status");
            Command::new(&edgedb)
                .arg("server").arg("status").arg("inst1")
                .assert()
                .success();

            println!("Stop");
            Command::new(&edgedb)
                .arg("server").arg("stop").arg("inst1")
                .assert()
                .success();

            println!("Status");
            Command::new(&edgedb)
                .arg("server").arg("status").arg("inst1")
                .assert()
                .code(3);

            println!("Init second one");
            Command::new(&edgedb)
                .arg("server").arg("init").arg("second")
                    .arg("--version=1-beta2")
                .assert()
                .success();

            println!("Start second");
            Command::new(&edgedb)
                .arg("server").arg("start").arg("second")
                .assert()
                .success();

            println!("Start first simultaneously to second");
            Command::new(&edgedb)
                .arg("server").arg("start").arg("inst1")
                .assert()
                .success();

            println!("Status second");
            Command::new(&edgedb)
                .arg("server").arg("status").arg("second")
                .assert()
                .success();

            println!("Logs inst1");
            Command::new(&edgedb)
                .arg("server").arg("logs").arg("inst1")
                .assert()
                .success();

            println!("Logs second");
            Command::new(&edgedb)
                .arg("server").arg("logs").arg("second")
                .assert()
                .success();

            println!("Status first");
            Command::new(&edgedb)
                .arg("server").arg("status").arg("inst1")
                .assert()
                .success();

            println!("Upgrading");
            Command::new(&edgedb)
                .arg("instance").arg("upgrade").arg("inst1")
                .arg("--to-latest").arg("--force")
                .assert()
                .success();

            println!("Execute query after upgrade");
            Command::new(&edgedb)
                .arg("--admin").arg("--instance").arg("inst1")
                .arg("--wait-until-available=20s")
                .arg("query").arg("SELECT 1")
                .assert()
                .success();

            println!("Delete second instance");
            Command::new(&edgedb)
                .arg("instance").arg("destroy").arg("second")
                .assert()
                .success();

        }

        println!("Uninstall the old version");
        Command::new(&edgedb)
            .arg("server").arg("uninstall").arg("--version=1-beta2")
            .assert()
            .success();
        Command::new(&edgedb)
            .arg("server").arg("list-versions")
            .arg("--installed-only").arg("--column=major-version")
            .assert()
            .success()
            .stdout(predicates::str::contains("1-beta2").not());

        if cfg!(target_os="macos") {
            println!("Execute query after deleting second");
            Command::new(&edgedb)
                .arg("--admin").arg("--instance").arg("inst1")
                .arg("--wait-until-available=20s")
                .arg("query").arg("SELECT 1")
                .assert()
                .success();
        }
    }

    Ok(())
}
