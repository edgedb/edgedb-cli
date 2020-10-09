#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tar::{Builder, Header};


pub struct Context {
    tar: Builder<Vec<u8>>,
}

pub fn sudoers() -> &'static str {
    r###"
        root        ALL=(ALL:ALL) SETENV: ALL
        user        ALL=(ALL:ALL) NOPASSWD: ALL
    "###
}

impl Context {
    pub fn new() -> Context {
        Context {
            tar: Builder::new(Vec::with_capacity(1048576)),
        }
    }
    pub fn add_file_mode(mut self, filename: impl AsRef<Path>,
        data: impl AsRef<[u8]>, mode: u32)
        -> anyhow::Result<Self>
    {
        let data = data.as_ref();
        let filename = filename.as_ref();
        let mut header = Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_path(filename)?;
        header.set_mode(mode);
        header.set_cksum();
        self.tar.append(&header, data)?;
        Ok(self)
    }
    pub fn add_file(self, filename: impl AsRef<Path>,
        data: impl AsRef<[u8]>)
        -> anyhow::Result<Self>
    {
        self.add_file_mode(filename, data, 0o644)
    }
    pub fn add_sudoers(self) -> anyhow::Result<Self> {
        self.add_file("sudoers", sudoers())
    }
    pub fn add_bin(self) -> anyhow::Result<Self> {
        self.add_file_mode("edgedb",
            fs::read(env!("CARGO_BIN_EXE_edgedb"))?,
            0o755)
    }
    pub fn build(mut self) -> anyhow::Result<Vec<u8>> {
        self.tar.finish()?;
        Ok(self.tar.into_inner()?)
    }
}

pub fn build_image(context: Context, tagname: &str) -> anyhow::Result<()> {
    Command::new("docker")
        .arg("build").arg("-")
        .arg("-t").arg(tagname)
        .write_stdin(context.build()?)
        .assert()
        .success();
    Ok(())
}

pub fn run_bg(container_name: &str, tagname: &str) -> std::process::Child {
    std::process::Command::new("docker")
        .arg("run")
        .arg("--rm")
        .arg("--name").arg(container_name)
        .arg(tagname)
        .spawn()
        .expect("can run docker command")
}

pub fn stop(container_name: &str) {
    Command::new("docker")
        .arg("stop")
        .arg(container_name)
        .assert();
}

pub fn run(tagname: &str, script: &str) -> assert_cmd::assert::Assert {
    Command::new("docker")
        .arg("run")
        .arg("--rm")
        .arg("-u").arg("1000")
        .arg(tagname)
        .args(&["sh", "-exc", script])
        .assert()
}

pub fn run_with_socket(tagname: &str, script: &str)
    -> assert_cmd::assert::Assert
{
    let path = if let Ok(path) = env::var("DOCKER_VOLUME_PATH") {
        path.to_string()
    } else {
        "/var/run/docker.sock".to_string()
    };
    Command::new("docker")
        .arg("run")
        .arg("--rm")
        .arg("-u").arg("1000")
        .arg(format!("--volume={0}:{0}", path))
        .arg("--net=host")
        .arg(tagname)
        .args(&["sh", "-exc", script])
        .assert()
}

pub fn run_with(tagname: &str, script: &str, link: &str)
    -> assert_cmd::assert::Assert
{
    Command::new("docker")
        .arg("run")
        .arg("--rm")
        .arg("-u").arg("1000")
        .arg(format!("--link={0}:{0}", link))
        .arg(tagname)
        .args(&["sh", "-exc", script])
        .assert()
}

pub fn sudo_test(dockerfile: &str, tagname: &str, nightly: bool)
    -> Result<(), anyhow::Error>
{
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, tagname)?;
    run(tagname, &format!(
            r###"
                RUST_LOG=info edgedb server install {arg}
                echo --- DONE ---
                /usr/bin/edgedb-server-* --version
            "###, arg=if nightly { "--nightly" } else {""})
        ).success()
        .stdout(predicates::str::contains("--- DONE ---"))
        .stdout(predicates::function::function(|data: &str| {
            let tail = &data[data.find("--- DONE ---").unwrap()..];
            assert!(tail.contains("edgedb-server, version"));
            true
        }));
    Ok(())
}

pub fn install_twice_test(dockerfile: &str, tagname: &str, nightly: bool)
    -> Result<(), anyhow::Error>
{
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, tagname)?;
    run(tagname, &format!(
            r###"
                RUST_LOG=info edgedb server install {arg}
                echo --- DONE --- 1>&2
                RUST_LOG=info edgedb server install {arg}
            "###, arg=if nightly { "--nightly" } else {""})
        ).code(51)
        .stderr(predicates::str::contains("--- DONE ---"))
        .stderr(predicates::function::function(|data: &str| {
            let tail = &data[data.find("--- DONE ---").unwrap()..];
            tail.contains("already installed")
        }));
    Ok(())
}

