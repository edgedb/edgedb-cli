use std::fs;
use std::io::Write;

use assert_cmd::Command;
use async_std::prelude::FutureExt;
use dirs::home_dir;
use tokio::sync::oneshot;
use warp::Filter;
use warp::filters::path::path;

use crate::certs::Certs;

const UNIX_INST: &str =
    "curl --proto '=https' --tlsv1.2 -sSf https://localhost:8443 | sh -s -- -y";

#[test]
fn github_action_install() -> anyhow::Result<()> {
    let mut tokio = tokio::runtime::Builder::new()
        .basic_scheduler()
        .threaded_scheduler()
        .core_threads(2)
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
        if cfg!(target_os="macos") {
            Command::new("sudo")
                .arg("security")
                .arg("add-trusted-cert")
                .arg("-d")
                .arg("-r").arg("trustRoot")
                .arg("-k").arg("/Library/Keychains/System.keychain")
                .arg(tmpfile.path())
                .assert()
                .success();
            Command::new("sh")
                .arg("-c")
                .arg("-e")
                .arg(UNIX_INST)
                .env("EDGEDB_PKG_ROOT", "https://localhost:8443")
                .assert()
                .success()
                .stdout(predicates::str::contains(
                    "EdgeDB command-line tool is installed now"));
        } else {
            Command::new("sh")
                .arg("-c")
                .arg("-e")
                .arg(UNIX_INST)
                .env("CURL_CA_BUNDLE", tmpfile.path())
                .env("EDGEDB_PKG_ROOT", "https://localhost:8443")
                .assert()
                .success()
                .stdout(predicates::str::contains(
                    "EdgeDB command-line tool is installed now"));
        }
    }

    shut_tx.send(()).ok();
    tokio.block_on(http)?;

    let edgedb = home_dir().unwrap()
        .join(".edgedb").join("bin").join("edgedb");

    Command::new(&edgedb)
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains(
            concat!("edgedb-cli ", env!("CARGO_PKG_VERSION"))));

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

        if cfg!(target_os="macos") {
            println!("Init default");
            Command::new(&edgedb)
                .arg("server").arg("init")
                .assert()
                .success();

            println!("Start");
            Command::new(&edgedb)
                .arg("server").arg("start")
                .assert()
                .success();

            println!("Status");
            Command::new(&edgedb)
                .arg("server").arg("status")
                .assert()
                .success();

            println!("Restart");
            Command::new(&edgedb)
                .arg("server").arg("restart")
                .assert()
                .success();

            println!("Status");
            Command::new(&edgedb)
                .arg("server").arg("status")
                .assert()
                .success();

            println!("Stop");
            Command::new(&edgedb)
                .arg("server").arg("stop")
                .assert()
                .success();

            println!("Status");
            Command::new(&edgedb)
                .arg("server").arg("status")
                .assert()
                .code(3);

            println!("Init second one");
            Command::new(&edgedb)
                .arg("server").arg("init").arg("second")
                .assert()
                .success();

            println!("Start second");
            Command::new(&edgedb)
                .arg("server").arg("start").arg("second")
                .assert()
                .success();

            println!("Start default simultaneously to second");
            Command::new(&edgedb)
                .arg("server").arg("start").arg("default")
                .assert()
                .success();

            println!("Status second");
            Command::new(&edgedb)
                .arg("server").arg("status").arg("second")
                .assert()
                .success();

            println!("Status default");
            Command::new(&edgedb)
                .arg("server").arg("status") // default
                .assert()
                .success();
        }
    }

    Ok(())
}
