#[cfg(not(windows))]
#[macro_use] extern crate pretty_assertions;

use std::sync::Mutex;
use std::convert::TryInto;
use std::io::{BufReader, BufRead};
use std::sync::mpsc::sync_channel;
use std::thread::{self, JoinHandle};
use std::process;
use std::env;

use assert_cmd::Command;
use once_cell::sync::Lazy;
use serde_json::from_str;

const DEFAULT_EDGEDB_VERSION: &str = "1-alpha4";

// Can't run server on windows
#[cfg(not(windows))]
mod dump_restore;
#[cfg(not(windows))]
mod configure;
#[cfg(not(windows))]
mod non_interactive;
#[cfg(not(windows))]
mod migrations;

// for some reason rexpect doesn't work on macos
// and also something wrong on musl libc
#[cfg(all(target_os="linux", not(target_env="musl")))]
mod interactive;


pub static SHUTDOWN_INFO: Lazy<Mutex<Vec<ShutdownInfo>>> =
    Lazy::new(|| Mutex::new(Vec::new()));
pub static SERVER: Lazy<ServerGuard> = Lazy::new(|| ServerGuard::start());

#[cfg(not(windows))]
#[test]
fn simple_query() {
    let cmd = SERVER.admin_cmd().arg("query").arg("SELECT 1+7").assert();
    cmd.success().stdout("8\n");
}

#[cfg(not(windows))]
#[test]
fn version() {
    let cmd = SERVER.admin_cmd().arg("--version").assert();
    cmd.success()
        .stdout(concat!("edgedb-cli ", env!("CARGO_PKG_VERSION"), "\n"));
}

pub struct ShutdownInfo {
    process: process::Child,
    thread: Option<JoinHandle<()>>,
}

pub struct ServerGuard {
    port: u16,
    runstate_dir: String,
}

impl ServerGuard {
    fn start() -> ServerGuard {
        use std::process::{Command, Stdio};

        let bin_name = format!("edgedb-server-{}",
            env::var("EDGEDB_MAJOR_VERSION")
            .expect(DEFAULT_EDGEDB_VERSION));
        let mut cmd = Command::new(&bin_name);
        cmd.arg("--temp-dir");
        cmd.arg("--testmode");
        cmd.arg("--echo-runtime-info");
        cmd.arg("--port=auto");
        cmd.arg("--default-database=edgedb");
        cmd.arg("--default-database-user=edgedb");
        #[cfg(unix)]
        if unsafe { libc::geteuid() } == 0 {
            use std::os::unix::process::CommandExt;
            // This is moslty true in vagga containers, so run edgedb/postgres
            // by any non-root user
            cmd.uid(1);
        }
        cmd.stdout(Stdio::piped());

        let mut process = cmd.spawn()
            .expect(&format!("Can run {}", bin_name));
        let process_in = process.stdout.take().expect("stdout is pipe");
        let (tx, rx) = sync_channel(1);
        let thread = thread::spawn(move || {
            let buf = BufReader::new(process_in);
            for line in buf.lines() {
                match line {
                    Ok(line) => {
                        if line.starts_with("EDGEDB_SERVER_DATA:") {
                            let data: serde_json::Value = from_str(
                                &line["EDGEDB_SERVER_DATA:".len()..])
                                .expect("valid server data");
                            println!("Server data {:?}", data);
                            let port = data.get("port")
                                .and_then(|x| x.as_u64())
                                .and_then(|x| x.try_into().ok())
                                .expect("valid server data");
                            let runstate_dir = data.get("runstate_dir")
                                .and_then(|x| x.as_str())
                                .map(|x| x.to_owned())
                                .expect("valid server data");
                            tx.send((port, runstate_dir))
                                .expect("valid channel");
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading from server: {}", e);
                        break;
                    }
                }
            }
        });
        let (port, runstate_dir) = rx.recv().expect("valid port received");

        let mut sinfo = SHUTDOWN_INFO.lock().expect("shutdown mutex works");
        if sinfo.is_empty() {
            shutdown_hooks::add_shutdown_hook(stop_processes);
        }
        sinfo.push(ShutdownInfo {
            process,
            thread: Some(thread),
        });

        ServerGuard {
            port,
            runstate_dir,
        }
    }

    pub fn admin_cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("edgedb").expect("binary found");
        cmd.arg("--no-version-check");
        cmd.arg("--admin");
        cmd.arg("--port").arg(self.port.to_string());
        cmd.env("EDGEDB_HOST", &self.runstate_dir);
        return cmd
    }

    #[cfg(not(windows))]
    pub fn admin_interactive(&self) -> rexpect::session::PtySession {
        use assert_cmd::cargo::CommandCargoExt;
        use rexpect::session::spawn_command;

        let mut cmd = process::Command::cargo_bin("edgedb")
            .expect("binary found");
        cmd.arg("--no-version-check");
        cmd.arg("--admin");
        cmd.arg("--port").arg(self.port.to_string());
        cmd.env("EDGEDB_HOST", &self.runstate_dir);
        return spawn_command(cmd, Some(5000)).expect("start interactive");
    }
    #[cfg(not(windows))]
    pub fn custom_interactive(&self, f: impl FnOnce(&mut process::Command))
        -> rexpect::session::PtySession
    {
        use assert_cmd::cargo::CommandCargoExt;
        use rexpect::session::spawn_command;

        let mut cmd = process::Command::cargo_bin("edgedb")
            .expect("binary found");
        cmd.arg("--no-version-check");
        cmd.arg("--admin");
        cmd.arg("--port").arg(self.port.to_string());
        cmd.env("EDGEDB_HOST", &self.runstate_dir);
        f(&mut cmd);
        return spawn_command(cmd, Some(5000)).expect("start interactive");
    }

    pub fn database_cmd(&self, database_name: &str) -> Command {
        let mut cmd = Command::cargo_bin("edgedb").expect("binary found");
        cmd.arg("--no-version-check");
        cmd.arg("--admin");
        cmd.arg("--port").arg(self.port.to_string());
        cmd.arg("--database").arg(database_name);
        cmd.env("EDGEDB_HOST", &self.runstate_dir);
        return cmd
    }
}


extern fn stop_processes() {
    let mut items = SHUTDOWN_INFO.lock().expect("shutdown mutex works");
    for item in items.iter_mut() {
        item.process.kill().ok();
    }
    for item in items.iter_mut() {
        item.process.wait().ok();
        item.thread.take().expect("not yet joined").join().ok();
    }
}
