use std::fs;
use std::path::{PathBuf, Path};
use std::process::{Command, exit};

use anyhow::Context;

use crate::process::{run, exit_from, get_text};
use crate::server::options::{Start, Stop, Restart, Status};
use crate::server::init::{data_path, Metadata};
use crate::server::methods::InstallMethod;
use crate::server::version::Version;
use crate::server::{linux, macos};
use crate::platform::{home_dir, get_current_uid};


pub trait Instance {
    fn start(&mut self, options: &Start) -> anyhow::Result<()>;
    fn stop(&mut self, options: &Stop) -> anyhow::Result<()>;
    fn restart(&mut self, options: &Restart) -> anyhow::Result<()>;
    fn status(&mut self, options: &Status) -> anyhow::Result<()>;
    fn get_socket(&self, admin: bool) -> anyhow::Result<PathBuf>;
}

pub struct SystemdInstance {
    name: String,
    #[allow(dead_code)]
    system: bool,
    version: Version<String>,
    data_dir: PathBuf,
    port: u16,
}

pub struct LaunchdInstance {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    system: bool,
    #[allow(dead_code)]
    version: Version<String>,
    unit_path: PathBuf,
    data_dir: PathBuf,
    port: u16,
}

pub fn get_instance(name: &str) -> anyhow::Result<Box<dyn Instance>> {
    let dir = data_path(false)?.join(name);
    if !dir.exists() {
        let sys_dir = data_path(true)?.join(name);
        if sys_dir.exists() {
            anyhow::bail!("System instances are not implemented yet");
        }
        anyhow::bail!("No instance {0:?} found. Run:\n  \
            edgedb server init {0}", name);
    }
    let metadata_path = dir.join("metadata.json");
    let metadata: Metadata = serde_json::from_slice(
        &fs::read(&metadata_path)
        .with_context(|| format!("failed to read metadata {}",
                                 metadata_path.display()))?)
        .with_context(|| format!("failed to read metadata {}",
                                 metadata_path.display()))?;
    match metadata.method {
        InstallMethod::Package if cfg!(target_os="linux") => {
            Ok(Box::new(SystemdInstance {
                name: name.to_owned(),
                system: false,
                version: metadata.version.to_owned(),
                port: metadata.port,
                data_dir: dir,
            }))
        }
        InstallMethod::Package if cfg!(target_os="macos") => {
            let unit_name = format!("com.edgedb.edgedb-server-{}.plist", name);
            Ok(Box::new(LaunchdInstance {
                name: name.to_owned(),
                system: false,
                version: metadata.version.to_owned(),
                data_dir: dir,
                unit_path: home_dir()?.join("Library/LaunchAgents")
                    .join(&unit_name),
                port: metadata.port,
            }))
        }
        _ => {
            anyhow::bail!("Unknown installation method and OS combination");
        }
    }
}

impl Instance for SystemdInstance {
    fn start(&mut self, options: &Start) -> anyhow::Result<()> {
        if options.foreground {
            let sock = self.get_socket(true)?;
            let socket_dir = sock.parent().unwrap();
            run(Command::new(linux::get_server_path(&self.version))
                .arg("--port").arg(self.port.to_string())
                .arg("--data-dir").arg(&self.data_dir)
                .arg("--runstate-dir").arg(&socket_dir))?;
        } else {
            run(Command::new("systemctl")
                .arg("--user")
                .arg("start")
                .arg(format!("edgedb-server@{}", self.name)))?;
        }
        Ok(())
    }
    fn stop(&mut self, _options: &Stop) -> anyhow::Result<()> {
        run(Command::new("systemctl")
            .arg("--user")
            .arg("stop")
            .arg(format!("edgedb-server@{}", self.name)))?;
        Ok(())
    }
    fn restart(&mut self, _options: &Restart) -> anyhow::Result<()> {
        run(Command::new("systemctl")
            .arg("--user")
            .arg("restart")
            .arg(format!("edgedb-server@{}", self.name)))?;
        Ok(())
    }
    fn status(&mut self, _options: &Status) -> anyhow::Result<()> {
        exit_from(Command::new("systemctl")
            .arg("--user")
            .arg("status")
            .arg(format!("edgedb-server@{}", self.name)))?;
        Ok(())
    }
    fn get_socket(&self, admin: bool) -> anyhow::Result<PathBuf> {
        Ok(dirs::runtime_dir()
            .unwrap_or_else(|| {
                Path::new("/run/user").join(get_current_uid().to_string())
            })
            .join(format!("edgedb-{}", self.name))
            .join(format!(".s.EDGEDB{}.{}",
                if admin { ".admin" } else { "" },
                self.port)))
    }
}

impl Instance for LaunchdInstance {
    fn start(&mut self, options: &Start) -> anyhow::Result<()> {
        if options.foreground {
            let sock = self.get_socket(true)?;
            let socket_dir = sock.parent().unwrap();
            run(Command::new(macos::get_server_path(&self.version)?)
                .arg("--port").arg(self.port.to_string())
                .arg("--data-dir").arg(&self.data_dir)
                .arg("--runstate-dir").arg(&socket_dir))?;
        } else {
            run(Command::new("launchctl")
                .arg("load").arg("-w")
                .arg(&self.unit_path))?;
        }
        Ok(())
    }
    fn stop(&mut self, _options: &Stop) -> anyhow::Result<()> {
        run(Command::new("launchctl")
            .arg("unload")
            .arg(&self.unit_path))?;
        Ok(())
    }
    fn restart(&mut self, _options: &Restart) -> anyhow::Result<()> {
        run(Command::new("launchctl")
            .arg("kickstart")
            .arg("-k")
            .arg(&format!("gui/{}/edgedb-server-{}",
                get_current_uid(), self.name)))?;
        Ok(())
    }
    fn status(&mut self, _options: &Status) -> anyhow::Result<()> {
        let services = get_text(Command::new("launchctl")
            .arg("list"))?;
        let svc_name = format!("edgedb-server-{}", self.name);
        for line in services.lines() {
            let mut iter = line.split_whitespace();
            let pid = iter.next().unwrap_or("-");
            let exit_code = iter.next().unwrap_or("<unknown>");
            let name = iter.next();
            if let Some(name) = name {
                if name == svc_name {
                    if pid == "-" {
                        eprintln!("Server exited with exit code {}",
                                  exit_code);
                        exit(3);
                    }
                    eprint!("Server is running, pid ");
                    println!("{}", pid);
                    return Ok(());
                }
            }
        }
        eprintln!("Server is not running");
        exit(3);
    }
    fn get_socket(&self, admin: bool) -> anyhow::Result<PathBuf> {
        Ok(home_dir()?
            .join(".edgedb/run")
            .join(&self.name)
            .join(format!(".s.EDGEDB{}.{}",
                if admin { ".admin" } else { "" },
                self.port)))
    }
}
