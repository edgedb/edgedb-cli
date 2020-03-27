use std::sync::Mutex;
use std::convert::TryInto;
use std::io::{BufReader, BufRead};
use std::process::Child;
use std::sync::mpsc::sync_channel;
use std::thread::{self, JoinHandle};

use assert_cmd::Command;
use serde_json::from_str;

mod dump_restore;


lazy_static::lazy_static! {
    pub static ref SERVER: ServerGuard = ServerGuard::start();
}

#[test]
fn simple_query() {
    let cmd = SERVER.admin_cmd().arg("query").arg("SELECT 1+7").assert();
    cmd.success().stdout("8\n");
}

pub struct ShutdownInfo {
    process: Child,
    thread: Option<JoinHandle<()>>,
}

pub struct ServerGuard {
    shutdown_info: Mutex<ShutdownInfo>,
    port: u16,
    runstate_dir: String,
}

impl ServerGuard {
    fn start() -> ServerGuard {
        use std::process::{Command, Stdio};

        let mut cmd = Command::new("edgedb-server");
        cmd.arg("--temp-dir");
        cmd.arg("--testmode");
        cmd.arg("--echo-runtime-info");
        cmd.arg("--port=auto");
        cmd.stdout(Stdio::piped());

        let mut process = cmd.spawn().expect("Can run edgedb-server");
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

        shutdown_hooks::add_shutdown_hook(stop_process);

        ServerGuard {
            shutdown_info: Mutex::new(ShutdownInfo {
                process,
                thread: Some(thread),
            }),
            port,
            runstate_dir,
        }
    }

    pub fn admin_cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("edgedb").expect("binary found");
        cmd.arg("--admin");
        cmd.arg("--port").arg(self.port.to_string());
        cmd.env("EDGEDB_HOST", &self.runstate_dir);
        return cmd
    }

    pub fn database_cmd(&self, database_name: &str) -> Command {
        let mut cmd = Command::cargo_bin("edgedb").expect("binary found");
        cmd.arg("--admin");
        cmd.arg("--port").arg(self.port.to_string());
        cmd.arg("--database").arg(database_name);
        cmd.env("EDGEDB_HOST", &self.runstate_dir);
        return cmd
    }
}


extern fn stop_process() {
    let mut sinfo = SERVER.shutdown_info.lock().expect("shutdown mutex works");
    sinfo.process.kill().ok();
    sinfo.process.wait().ok();
    sinfo.thread.take().expect("not yet joined").join().ok();
}
