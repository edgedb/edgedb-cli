use std::convert::TryInto;
use std::io::{BufReader, BufRead};
use std::process::Child;
use std::sync::mpsc::sync_channel;
use std::thread::{self, JoinHandle};

use assert_cmd::Command;
use serde_json::from_str;


lazy_static::lazy_static! {
    pub static ref SERVER: ServerGuard = ServerGuard::start();
}

#[test]
fn simple_query() {
    let guard = &*SERVER;
    let cmd = Command::cargo_bin("edgedb").expect("binary found")
        .arg("--admin")
        .arg("--port").arg(guard.port.to_string())
        .arg("query").arg("SELECT 1+7")
        .env("EDGEDB_HOST", &guard.runstate_dir)
        .assert();
    cmd.success().stdout("8\n");
}

pub struct ServerGuard {
    process: Child,
    thread: Option<JoinHandle<()>>,
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
        cmd.arg("--auto-shutdown");
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
        ServerGuard {
            process,
            thread: Some(thread),
            port,
            runstate_dir,
        }
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.process.kill().ok();
        self.thread.take().expect("not yet joined").join().ok();
    }
}
