#![cfg_attr(not(feature="test_docker_wrapper"),
            allow(dead_code, unused_imports))]

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::fs;

use assert_cmd::Command;
use once_cell::sync::Lazy;
use test_case::test_case;

mod util;
mod docker;

use util::*;


#[derive(serde::Deserialize)]
struct Artifact {
    target: Target,
    executable: String,
}

#[derive(serde::Deserialize)]
struct Target {
    name: String,
    test: bool,
}

static TEST_EXECUTABLES: Lazy<HashMap<String, PathBuf>> = Lazy::new(|| {

    let tests = std::process::Command::new("cargo")
        .arg("build")
        .arg("--tests")
        .arg("--features=docker_test_wrapper,portable_tests")
        .arg("--message-format=json")
        .output().unwrap();
    let mut executables: HashMap<String, PathBuf> = HashMap::new();
    for line in tests.stdout.split(|&c| c == b'\n') {
        let art = match serde_json::from_slice::<Artifact>(line) {
            Ok(art) if
                art.target.name.starts_with("portable") && art.target.test
            => art,
            Ok(_) | Err(_) => continue,
        };
        executables.insert(art.target.name.clone(), art.executable.into());
    }
    assert!(executables.len() > 0);

    let mut context = docker::Context::new();
    context = context.add_file("Dockerfile", dockerfile()).unwrap();
    context = context.add_bin().unwrap();
    context = context.add_dir("tests/proj", "tests/proj").unwrap();
    let base = Path::new("tests/");
    for path in executables.values() {
        if path.extension().is_some() {
            continue;
        }
        if let Some(name) = path.file_name() {
            context = context.add_file_mode(
                base.join(name),
                fs::read(path).unwrap(),
                0x755,
            ).unwrap();
        }
    }

    docker::build_image(context, "edgedb_test_portable").unwrap();
    shutdown_hooks::add_shutdown_hook(delete_docker_image);

    return executables;
});

extern fn delete_docker_image() {
    std::process::Command::new("docker")
        .arg("image")
        .arg("rm")
        .arg("edgedb_test_portable")
        .output()
        .map_or_else(
            |e| println!("docker image rm failed: {:?}", e),
            |o| {
                if !o.status.success() {
                    println!("docker image rm failed: {:?}", o)
                }
            },
        );
}

fn dockerfile() -> String {
    format!(r###"
        FROM ubuntu:focal
        ENV DEBIAN_FRONTEND=noninteractive
        RUN apt-get update && apt-get install -y \
            ca-certificates sudo gnupg2 apt-transport-https curl \
            software-properties-common dbus-user-session
        RUN adduser --uid 1000 --home /home/user1 \
            --shell /bin/bash --ingroup users --gecos "EdgeDB Test User" \
            user1
        RUN mkdir /home/edgedb && chown user1 /home/edgedb
        ADD ./edgedb /usr/bin/edgedb
        ADD ./tests /tests
        RUN chown -R user1 /tests/proj
    "###)
}

#[cfg(feature="docker_test_wrapper")]
#[test_case("portable_smoke")]
#[test_case("portable_project")]
#[test_case("portable_project_dir")]
#[test_case("portable_shared")]
fn run_test(name: &'static str) {
    let file_name = TEST_EXECUTABLES.get(name).unwrap()
        .file_name().unwrap()
        .to_str().unwrap();

    let script = format!(r###"
        export XDG_RUNTIME_DIR=/run/user/1000
        export EDGEDB_INSTALL_IN_DOCKER=allow
        export RUST_TEST_THREADS=1

        /lib/systemd/systemd --user &
        exec /tests/{file_name}
    "###, file_name=file_name);

    let script = format!(r###"
        cg_path=$(cat /proc/self/cgroup | grep -oP '(?<=name=).*' | sed s/://)
        mkdir -p /run/user/1000 /sys/fs/cgroup/$cg_path
        chown user1 /sys/fs/cgroup/$cg_path /run/user/1000
        sudo -H -u user1 bash -exc {script}
    "###, script=shell_escape::escape(script.into()));

    Command::new("docker")
        .arg("run")
        .arg("--rm")
        .arg("--tmpfs=/run")
        .arg("--tmpfs=/run/systemd/system")
        .arg("--privileged")
        .arg("edgedb_test_portable")
        .args(&["sh", "-exc", &script])
        .assert()
        .context(name, "running test in docker")
        .success();
}
