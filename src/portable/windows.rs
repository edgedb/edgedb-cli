#![cfg_attr(not(windows), allow(unused_imports, dead_code))]

use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, Duration};

use anyhow::Context;
use fn_error_context::context;
use libflate::gzip;
use once_cell::sync::{Lazy, OnceCell};
use url::Url;

use crate::bug;
use crate::cli::upgrade::{self, self_version};
use crate::commands::ExitCode;
use crate::credentials;
use crate::hint::HintExt;
use crate::platform::{cache_dir, wsl_dir, config_dir, tmp_file_path};
use crate::portable::control;
use crate::portable::destroy;
use crate::portable::exit_codes;
use crate::portable::local::{InstanceInfo, Paths, write_json, NonLocalInstance};
use crate::portable::options::{self, Logs, StartConf, instance_arg, InstanceName};
use crate::portable::project;
use crate::portable::repository::{self, download, PackageHash, PackageInfo};
use crate::portable::status::{self, Service};
use crate::portable::ver;
use crate::print::{self, echo, Highlight};
use crate::process;


const CURRENT_DISTRO: &str = "EdgeDB.WSL.1";
const DISTRO_URL: Lazy<Url> = Lazy::new(|| {
    "https://aka.ms/wsl-debian-gnulinux".parse().expect("wsl url parsed")
});
const CERT_UPDATE_INTERVAL: Duration = Duration::from_secs(30*86400);
const IS_IN_WSL: Lazy<bool> = Lazy::new(|| {
    if cfg!(target_os="linux") {
        fs::read_to_string("/proc/version")
            .map(|s| s.contains("Microsoft"))
            .unwrap_or(false)
    } else {
        false
    }
});

static WSL: OnceCell<Wsl> = OnceCell::new();

#[derive(Debug, thiserror::Error)]
#[error("WSL distribution is not installed")]
pub struct NoDistribution;

struct Wsl {
    #[cfg(windows)]
    #[allow(dead_code)]
    lib: wslapi::Library,
    distribution: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct WslInfo {
    distribution: String,
    last_checked_version: Option<ver::Semver>,
    #[serde(skip_serializing_if="Option::is_none")]
    cli_timestamp: Option<SystemTime>,
    cli_version: ver::Semver,
    certs_timestamp: SystemTime,
}

impl Wsl {
    fn edgedb(&self) -> process::Native {
        let mut pro = process::Native::new("edgedb", "edgedb", "wsl");
        pro.arg("--user").arg("edgedb");
        pro.arg("--distribution").arg(&self.distribution);
        pro.arg("_EDGEDB_FROM_WINDOWS=1");
        if let Some(log_env) = env::var_os("RUST_LOG") {
            let mut pair =
                OsString::with_capacity("RUST_LOG=".len() + log_env.len());
            pair.push("RUST_LOG=");
            pair.push(log_env);
            pro.arg(pair);
        }
        pro.arg("/usr/bin/edgedb");
        pro.no_proxy();
        return pro
    }
    #[cfg(windows)]
    fn copy_out(&self, src: impl AsRef<str>, destination: impl AsRef<Path>)
        -> anyhow::Result<()>
    {
        let dest = path_to_linux(destination.as_ref())?;
        let cmd = format!("cp {} {}",
                            shell_escape::unix::escape(src.as_ref().into()),
                            shell_escape::unix::escape(dest.into()));

        let code = self.lib.launch_interactive(
            &self.distribution,
            &cmd,
            /* current_working_dir */ false,
        )?;
        if code != 0 {
            anyhow::bail!("WSL command {:?} exited with exit code: {}",
                          cmd, code);
        }
        Ok(())
    }

    fn read_text_file(&self, linux_path: impl AsRef<str>)
        -> anyhow::Result<String>
    {
        process::Native::new("read file", "wsl", "wsl")
            .arg("--user").arg("edgedb")
            .arg("--distribution").arg(&self.distribution)
            .arg("cat")
            .arg(linux_path.as_ref())
            .get_stdout_text()
    }

    fn check_path_exist(&self, linux_path: impl AsRef<str>) -> bool
    {
        process::Native::new("ls file", "wsl", "wsl")
            .arg("--user").arg("edgedb")
            .arg("--distribution").arg(&self.distribution)
            .arg("ls")
            .arg(linux_path.as_ref())
            .run()
            .is_ok()
    }

    #[cfg(not(windows))]
    fn copy_out(&self, _src: impl AsRef<str>, _destination: impl AsRef<Path>)
        -> anyhow::Result<()>
    {
        unreachable!();
    }

}

fn credentials_linux(instance: &str) -> String {
    format!("/home/edgedb/.config/edgedb/credentials/{}.json", instance)
}

#[context("cannot convert to linux (WSL) path {:?}", path)]
fn path_to_linux(path: &Path) -> anyhow::Result<String> {
    use std::path::Component::*;
    use std::path::Prefix::*;
    if !path.is_absolute() {
        return Err(bug::error("path must be absolute"))?;
    }

    let mut result = String::with_capacity(
        path.to_str().map(|m| m.len()).unwrap_or(32) + 32);
    result.push_str("/mnt");
    for component in path.components() {
        match component {
            Prefix(pre) => match pre.kind() {
                VerbatimDisk(c) | Disk(c) if c.is_ascii_alphabetic() => {
                    result.push('/');
                    result.push((c as char).to_ascii_lowercase());
                }
                _ => anyhow::bail!("unsupported prefix {:?}", pre),
            },
            RootDir => {}
            CurDir => return Err(bug::error("current dir in canonical path")),
            ParentDir => return Err(bug::error("parent dir in canonical path")),
            Normal(s) => {
                result.push('/');
                result.push_str(
                    s.to_str().context("invalid characters in path")?,
                );
            }
        }
    }
    Ok(result)
}

pub fn create_instance(options: &options::Create, name: &str,
                       port: u16, paths: &Paths)
    -> anyhow::Result<()>
{
    let wsl = ensure_wsl()?;

    let inner_options = options::Create {
        port: Some(port),
        ..options.clone()
    };
    wsl.edgedb()
        .arg("instance").arg("create").args(&inner_options)
        .run()?;

    if let Some(dir) = paths.credentials.parent() {
        fs_err::create_dir_all(&dir)?;
    }
    wsl.copy_out(credentials_linux(&name), &paths.credentials)?;

    Ok(())
}

pub fn destroy(options: &options::Destroy) -> anyhow::Result<()> {
    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => name,
        InstanceName::Cloud { .. } => todo!(),
    };
    let mut found = false;
    if let Some(wsl) = get_wsl()? {
        let options = options::Destroy {
            non_interactive: true,
            quiet: true,
            .. options.clone()
        };
        let status = wsl.edgedb()
            .arg("instance").arg("destroy").args(&options)
            .status()?;
        match status.code() {
            Some(exit_codes::INSTANCE_NOT_FOUND) => {}
            Some(0) => found = true,
            Some(c) => return Err(ExitCode::new(c).into()),
            None => anyhow::bail!("Interrupted"),
        }
    }

    let paths = Paths::get(name)?;
    if paths.credentials.exists() {
        found = true;
        log::info!(target: "edgedb::portable::destroy",
                   "Removing credentials file {:?}", &paths.credentials);
        fs::remove_file(&paths.credentials)?;
    }
    for path in &paths.service_files {
        if path.exists() {
            found = true;
            log::info!(target: "edgedb::portable::destroy",
                       "Removing service file {:?}", path);
            fs::remove_file(path)?;
        }
    }
    if !found {
        echo!("No instance named", name.emphasize(), "found");
        return Err(ExitCode::new(exit_codes::INSTANCE_NOT_FOUND).into());
    }
    Ok(())
}

#[context("cannot read {:?}", path)]
fn read_wsl(path: &Path) -> anyhow::Result<WslInfo> {
    let file = io::BufReader::new(fs::File::open(path)?);
    Ok(serde_json::from_reader(file)?)
}

#[context("cannot unpack debian distro from {:?}", zip_path)]
fn unpack_appx(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
    let mut zip = zip::ZipArchive::new(
        io::BufReader::new(fs::File::open(&zip_path)?))?;
    let name = zip.file_names()
        .find(|name| {
            let lower = name.to_lowercase();
            lower.starts_with("distrolauncher-") &&
            lower.ends_with("_x64.appx")
        })
        .ok_or_else(||anyhow::anyhow!(
            "file `DistroLauncher-*_x64.appx` is not found in archive"))?
        .to_string();
    let mut inp = zip.by_name(&name)?;
    let mut out = fs::File::create(dest)?;
    io::copy(&mut inp, &mut out)?;
    Ok(())
}

#[context("cannot unpack root filesystem from {:?}", zip_path)]
fn unpack_root(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
    let mut zip = zip::ZipArchive::new(
        io::BufReader::new(fs::File::open(&zip_path)?))?;
    let name = zip.file_names()
        .find(|name| name.eq_ignore_ascii_case("install.tar.gz"))
        .ok_or_else(|| anyhow::anyhow!(
            "file `install.tar.gz` is not found in archive"))?
        .to_string();
    let mut inp = gzip::Decoder::new(
        io::BufReader::new(zip.by_name(&name)?))?;
    let mut out = fs::File::create(dest)?;
    io::copy(&mut inp, &mut out)?;
    Ok(())
}

#[cfg(windows)]
fn wsl_check_cli(_wsl: &wslapi::Library, wsl_info: &WslInfo)
    -> anyhow::Result<bool>
{
    let self_ver = self_version()?;
    Ok(wsl_info.last_checked_version.as_ref()
       .map(|v| v != &self_ver).unwrap_or(true))
}

#[cfg(windows)]
#[context("cannot check linux CLI version")]
fn wsl_cli_version(distro: &str)
    -> anyhow::Result<ver::Semver>
{
    // Note: cannot capture output using wsl.launch
    let data = process::Native::new("check version", "edgedb", "wsl")
        .arg("--user").arg("edgedb")
        .arg("--distribution").arg(distro)
        .arg("/usr/bin/edgedb")
        .arg("--version")
        .get_stdout_text()?;
    let version = data.trim().strip_prefix("EdgeDB CLI ")
        .with_context(|| format!(
                "bad version info returned by linux CLI: {:?}", data))?
        .parse()?;
    Ok(version)
}

#[cfg(windows)]
fn download_binary(dest: &Path) -> anyhow::Result<()> {
    let my_ver = self_version()?;
    let pkgs = repository::get_platform_cli_packages(
        upgrade::channel(), "x86_64-unknown-linux-musl")?;
    let pkg = pkgs.iter().find(|pkg| pkg.version == my_ver);
    let pkg = if let Some(pkg) = pkg {
        pkg.clone()
    } else {
        let pkg = repository::get_platform_cli_packages(
                upgrade::channel(),
                "x86_64-unknown-linux-musl",
            )?.into_iter().max_by(|a, b| a.version.cmp(&b.version))
            .context("cannot find new version")?;
        if pkg.version < my_ver {
            return Err(bug::error(format!(
                "latest version of linux CLI {} \
                 is older that current windows CLI {}",
                 pkg.version,
                 my_ver)));
        }
        log::warn!("No matching package version {} found. \
                    Using latest one {}.",
                    my_ver, pkg.version);
        pkg
    };

    let down_path = dest.with_extension("download");
    let tmp_path = tmp_file_path(&dest);
    download(&down_path, &pkg.url, false)?;
    upgrade::unpack_file(&down_path, &tmp_path, pkg.compression)?;
    fs_err::rename(&tmp_path, dest)?;

    Ok(())
}

#[cfg(windows)]
fn wsl_simple_cmd(wsl: &wslapi::Library, distro: &str, cmd: &str)
    -> anyhow::Result<()>
{
    let code = wsl.launch_interactive(
        distro,
        cmd,
        /* current_working_dir */ false,
    )?;
    if code != 0 {
        anyhow::bail!("WSL command {:?} exited with exit code: {}",
                      cmd, code);
    }
    Ok(())
}

fn utf16_contains(bytes: &[u8], needle: &str) -> bool {
    use std::char::{decode_utf16, REPLACEMENT_CHARACTER};
    decode_utf16(bytes.chunks_exact(2)
                 .map(|a| u16::from_le_bytes([a[0], a[1]])))
       .map(|r| r.unwrap_or(REPLACEMENT_CHARACTER))
       .collect::<String>()
       .contains(needle)
}

#[cfg(windows)]
#[context("cannot initialize WSL2 (windows subsystem for linux)")]
fn get_wsl_distro(install: bool) -> anyhow::Result<Wsl> {
    let wsl = wslapi::Library::new()?;
    let meta_path = config_dir()?.join("wsl.json");
    let mut distro = None;
    let mut update_cli = true;
    let mut certs_timestamp = None;
    if meta_path.exists() {
        match read_wsl(&meta_path) {
            Ok(wsl_info)
            if wsl.is_distribution_registered(&wsl_info.distribution)
            => {
                update_cli = wsl_check_cli(&wsl, &wsl_info)?;
                let update_certs = wsl_info.certs_timestamp + CERT_UPDATE_INTERVAL
                    < SystemTime::now();
                if !update_cli && !update_certs {
                    return Ok(Wsl {
                        lib: wsl,
                        distribution: wsl_info.distribution,
                    });
                }
                if !update_certs {
                    certs_timestamp = Some(wsl_info.certs_timestamp);
                }
                distro = Some(wsl_info.distribution);
            }
            Ok(_) => {}
            Err(e) => {
                log::warn!("Error reading WLS metadata: {:#}", e);
            }
        }
    }
    let mut distro = distro.unwrap_or(CURRENT_DISTRO.to_string());

    let download_dir = cache_dir()?.join("downloads");
    fs::create_dir_all(&download_dir)?;

    if !wsl.is_distribution_registered(&distro) {
        update_cli = true;
        certs_timestamp = None;
        if !install {
            return Err(NoDistribution.into());
        }

        if let Ok(use_distro) = env::var("_EDGEDB_WSL_DISTRO") {
            distro = use_distro;
        } else {
            let download_dir = cache_dir()?.join("downloads");
            fs::create_dir_all(&download_dir)?;

            let download_path = download_dir.join("debian.zip");
            download(&download_path, &*DISTRO_URL, false)?;
            echo!("Unpacking WSL distribution...");
            let appx_path = download_dir.join("debian.appx");
            unpack_appx(&download_path, &appx_path)?;
            let root_path = download_dir.join("install.tar");
            unpack_root(&appx_path, &root_path)?;

            let distro_path = wsl_dir()?.join(CURRENT_DISTRO);
            fs::create_dir_all(&distro_path)?;
            echo!("Initializing WSL distribution...");

            let result = process::Native::new("wsl check", "wsl", "wsl")
                .arg("--help")
                .get_output();

            match result {
                Ok(out) if !utf16_contains(&out.stdout, "--import") => {
                    return Err(anyhow::anyhow!(
                        "WSL currently installed is outdated."))
                        .hint("Please run `wsl --install` under \
                               administrator privileges for the upgrade.")?;
                }
                Ok(_) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Error running `wsl` tool: {:#}", e))
                        .hint("You must be running Windows 10 version 2004 \
                               and higher (Build 19041 and higher) \
                               or Windows 11.")?;
                }
            }

            process::Native::new("wsl import", "wsl", "wsl")
                .arg("--import")
                .arg(CURRENT_DISTRO)
                .arg(&distro_path)
                .arg(&root_path)
                .arg("--version=2")
                .run()?;

            fs::remove_file(&download_path)?;
            fs::remove_file(&appx_path)?;
            fs::remove_file(&root_path)?;

            distro = CURRENT_DISTRO.into();
        };

        wsl_simple_cmd(&wsl, &distro,
                       "useradd edgedb --uid 1000 --create-home")?;
    }

    if update_cli {
        echo!("Updating container's CLI version...");
        if let Some(bin_path) = env::var_os("_EDGEDB_WSL_LINUX_BINARY") {
            let bin_path = fs::canonicalize(bin_path)?;
            wsl_simple_cmd(&wsl, &distro, &format!(
                "cp {} /usr/bin/edgedb && chmod 755 /usr/bin/edgedb",
                shell_escape::unix::escape(path_to_linux(&bin_path)?.into()),
            ))?;
        } else {
            let cache_path = download_dir.join("edgedb");
            download_binary(&cache_path)?;
            wsl_simple_cmd(&wsl, &distro, &format!(
                "mv {} /usr/bin/edgedb && chmod 755 /usr/bin/edgedb",
                shell_escape::unix::escape(path_to_linux(&cache_path)?.into()),
            ))?;
        };
    }

    let certs_timestamp = if let Some(ts) = certs_timestamp {
        ts
    } else {
        echo!("Checking certificate updates...");
        process::Native::new("update certificates", "apt", "wsl")
            .arg("--distribution").arg(&distro)
            .arg("bash").arg("-c")
            .arg("export DEBIAN_FRONTEND=noninteractive; \
                  apt-get update -qq && \
                  apt-get install -y ca-certificates -qq -o=Dpkg::Use-Pty=0 && \
                  apt-get clean -qq && \
                  rm -rf /var/lib/apt/lists/*")
            .run()?;
        SystemTime::now()
    };

    let cli_version = wsl_cli_version(&distro)?;
    let my_ver = self_version()?;
    if cli_version < my_ver {
        return Err(bug::error(format!(
            "could not download correct version of CLI tools; \
            downloaded {}, expected {}", cli_version, my_ver)));
    }
    let info = WslInfo {
        distribution: distro.into(),
        last_checked_version: Some(my_ver),
        cli_timestamp: None,
        cli_version,
        certs_timestamp,
    };
    write_json(&meta_path, "WSL info", &info)?;
    return Ok(Wsl {
        lib: wsl,
        distribution: info.distribution,
    });
}

#[cfg(unix)]
fn get_wsl_distro(_install: bool) -> anyhow::Result<Wsl> {
    Err(bug::error("WSL on unix is unupported"))
}

fn ensure_wsl() -> anyhow::Result<&'static Wsl> {
    WSL.get_or_try_init(|| get_wsl_distro(true))
}

fn get_wsl() -> anyhow::Result<Option<&'static Wsl>> {
    match WSL.get_or_try_init(|| get_wsl_distro(false)) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is::<NoDistribution>() => Ok(None),
        Err(e) => Err(e),
    }
}

fn try_get_wsl() -> anyhow::Result<&'static Wsl> {
    match WSL.get_or_try_init(|| get_wsl_distro(false)) {
        Ok(v) => Ok(v),
        Err(e) if e.is::<NoDistribution>() => {
            return Err(e).hint("WSL is initialized automatically on \
              `edgedb project init` or `edgedb instance create`")?;
        }
        Err(e) => Err(e),
    }
}

pub fn startup_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir().context("cannot determine data directory")?
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup"))
}

fn service_file(instance: &str) -> anyhow::Result<PathBuf> {
    Ok(startup_dir()?.join(format!("edgedb-server-{}.cmd", instance)))
}

pub fn service_files(name: &str) -> anyhow::Result<Vec<PathBuf>> {
    Ok(vec![ service_file(name)? ])
}

pub fn create_service(info: &InstanceInfo) -> anyhow::Result<()> {
    let wsl = try_get_wsl()?;
    create_and_start(wsl, &info.name)
}

fn create_and_start(wsl: &Wsl, name: &str) -> anyhow::Result<()> {
    wsl.edgedb().arg("instance").arg("start").arg("-I").arg(&name).run()?;
    fs_err::write(service_file(&name)?, format!("wsl \
        --distribution {} --user edgedb \
        /usr/bin/edgedb instance start -I {}",
        &wsl.distribution, &name))?;
    Ok(())
}

pub fn stop_and_disable(_name: &str) -> anyhow::Result<bool> {
    anyhow::bail!("running as a service is not supported on Windows yet");
}

pub fn server_cmd(instance: &str, _is_shutdown_supported: bool)
    -> anyhow::Result<process::Native>
{
    let wsl = try_get_wsl()?;
    let mut pro = wsl.edgedb();
    pro.arg("instance").arg("start").arg("--foreground")
        .arg("-I").arg(instance);
    let instance = String::from(instance);
    pro.stop_process(move || {
        let mut cmd = async_process::Command::new("wsl");
        cmd.arg("--user").arg("edgedb");
        cmd.arg("--distribution").arg(&wsl.distribution);
        cmd.arg("_EDGEDB_FROM_WINDOWS=1");
        cmd.arg("/usr/bin/edgedb");
        cmd.arg("instance").arg("stop").arg("-I").arg(&instance);
        cmd
    });
    Ok(pro)
}

pub fn daemon_start(instance: &str) -> anyhow::Result<()> {
    let wsl = try_get_wsl()?;
    wsl.edgedb()
        .arg("instance").arg("start").arg("-I").arg(&instance)
        .no_proxy().run()?;
    Ok(())
}

pub fn start_service(_instance: &str) -> anyhow::Result<()> {
    anyhow::bail!("running as a service is not supported on Windows yet");
}

pub fn stop_service(_name: &str) -> anyhow::Result<()> {
    anyhow::bail!("running as a service is not supported on Windows yet");
}

pub fn restart_service(_inst: &InstanceInfo) -> anyhow::Result<()> {
    anyhow::bail!("running as a service is not supported on Windows yet");
}

pub fn service_status(_inst: &str) -> Service {
    Service::Inactive {
        error: "running as a service is not supported on Windows yet".into(),
    }
}

pub fn external_status(_inst: &InstanceInfo) -> anyhow::Result<()> {
    anyhow::bail!("running as a service is not supported on Windows yet");
}

pub fn is_wrapped() -> bool {
    env::var_os("_EDGEDB_FROM_WINDOWS").map(|x| !x.is_empty()).unwrap_or(false)
}

pub fn install(options: &options::Install) -> anyhow::Result<()> {
    ensure_wsl()?
        .edgedb()
        .arg("server").arg("install").args(options)
        .run()?;
    Ok(())
}

pub fn uninstall(options: &options::Uninstall) -> anyhow::Result<()> {
    if let Some(wsl) = get_wsl()? {
        wsl.edgedb()
            .arg("server").arg("uninstall").args(options)
            .run()?;
    } else {
        log::warn!("WSL distribution is not installed, \
                   so no EdgeDB server versions are present.");
    }
    Ok(())
}

pub fn list_versions(options: &options::ListVersions) -> anyhow::Result<()> {
    if let Some(wsl) = get_wsl()? {
        wsl.edgedb()
            .arg("server").arg("list-versions").args(options)
            .run()?;
    } else if options.json {
        println!("[]");
    } else {
        log::warn!("WSL distribution is not installed, \
                   so no EdgeDB server versions are present.");
    }
    Ok(())
}

pub fn info(options: &options::Info) -> anyhow::Result<()> {
    if let Some(wsl) = get_wsl()? {
        wsl.edgedb()
            .arg("server").arg("info").args(options)
            .run()?;
    } else {
        anyhow::bail!("WSL distribution is not installed, \
                       so no EdgeDB server versions are present.");
    }
    Ok(())
}

pub fn reset_password(options: &options::ResetPassword) -> anyhow::Result<()> {
    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => name,
        InstanceName::Cloud { .. } => todo!(),
    };
    if let Some(wsl) = get_wsl()? {
        wsl.edgedb()
            .arg("instance").arg("reset-password").args(options)
            .run()?;
        wsl.copy_out(credentials_linux(name),
                     credentials::path(name)?)?;
    } else {
        anyhow::bail!("WSL distribution is not installed, \
                       so no EdgeDB instances are present.");
    }
    Ok(())
}

pub fn start(options: &options::Start) -> anyhow::Result<()> {
    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => name,
        InstanceName::Cloud { .. } => todo!(),
    };
    if let Some(wsl) = get_wsl()? {
        if options.foreground {
            wsl.edgedb().arg("instance").arg("start").args(options).run()?;
        } else {
            create_and_start(wsl, name)?;
        }
    } else {
        anyhow::bail!("WSL distribution is not installed, \
                       so no EdgeDB instances are present.");
    }
    Ok(())
}

pub fn stop(options: &options::Stop) -> anyhow::Result<()> {
    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => name,
        InstanceName::Cloud { .. } => todo!(),
    };
    if let Some(wsl) = get_wsl()? {
        let service_file = service_file(name)?;
        fs::remove_file(&service_file)
            .map_err(|e| log::warn!("error removing {service_file:?}: {e:#}"))
            .ok();
        wsl.edgedb()
            .arg("instance").arg("stop").args(options)
            .run()?;
    } else {
        anyhow::bail!("WSL distribution is not installed, \
                       so no EdgeDB instances are present.");
    }
    Ok(())
}

pub fn restart(options: &options::Restart) -> anyhow::Result<()> {
    if let Some(wsl) = get_wsl()? {
        wsl.edgedb()
            .arg("instance").arg("restart").args(options)
            .run()?;
    } else {
        anyhow::bail!("WSL distribution is not installed, \
                       so no EdgeDB instances are present.");
    }
    Ok(())
}

pub fn logs(options: &options::Logs) -> anyhow::Result<()> {
    if let Some(wsl) = get_wsl()? {
        wsl.edgedb()
            .arg("instance").arg("logs").args(options)
            .run()?;
    } else {
        anyhow::bail!("WSL distribution is not installed, \
                       so no EdgeDB instances are present.");
    }
    Ok(())
}

pub fn status(options: &options::Status) -> anyhow::Result<()> {
    if options.service {
        if let Some(wsl) = get_wsl()? {
            wsl.edgedb()
                .arg("instance").arg("status").args(options)
                .run()?;
        } else {
            echo!("WSL distribution is not installed, \
                   so no EdgeDB instances are present.");
            return Err(ExitCode::new(exit_codes::INSTANCE_NOT_FOUND).into());
        }
    } else {
        let inner_opts = options::Status {
            quiet: true,
            .. options.clone()
        };
        if let Some(wsl) = get_wsl()? {
            let status = wsl.edgedb()
                .arg("instance").arg("status").args(&inner_opts)
                .status()?;
            match status.code() {
                Some(exit_codes::INSTANCE_NOT_FOUND) => {}
                Some(0) => return Ok(()),
                Some(c) => return Err(ExitCode::new(c).into()),
                None => anyhow::bail!("Interrupted"),
            }
        } // else can only be remote instance
        status::remote_status(options)?;
    }
    Ok(())
}

pub fn list(options: &options::List, opts: &crate::Options) -> anyhow::Result<()> {
    if options.debug || options.extended {
        let inner_opts = options::List {
            quiet: true,
            no_remote: true,
            .. options.clone()
        };
        if let Some(wsl) = get_wsl()? {
            wsl.edgedb()
                .arg("instance").arg("list").args(&inner_opts)
                .run()?;
        }
    }
    let inner_opts = options::List {
        no_remote: true,
        extended: false,
        debug: false,
        json: true,
        .. options.clone()
    };
    let local: Vec<status::JsonStatus> = if let Some(wsl) = get_wsl()? {
        let text = wsl.edgedb()
            .arg("instance").arg("list").args(&inner_opts)
            .get_stdout_text()?;
        log::info!("WSL list returned {:?}", text);
        serde_json::from_str(&text)
            .context("cannot decode json from edgedb CLI in WSL")?
    } else {
        Vec::new()
    };
    let visited = local.iter()
        .map(|v| v.name.clone())
        .collect::<BTreeSet<_>>();

    let remote = if options.no_remote {
        Vec::new()
    } else {
        status::get_remote(&visited, opts)?
    };

    if local.is_empty() && remote.is_empty() {
        if options.json {
            println!("[]");
        } else if !options.quiet {
            print::warn("No instances found");
        }
        return Ok(());
    }
    if options.debug {
        for status in remote {
            println!("{:#?}", status);
        }
    } else if options.extended {
        for status in remote {
            status.print_extended();
        }
    } else if options.json {
        println!("{}", serde_json::to_string_pretty(
            &local.into_iter()
            .chain(remote.iter().map(|status| status.json()))
            .collect::<Vec<_>>()
        )?);
    } else {
        status::print_table(&local, &remote);
    }

    Ok(())
}

pub fn upgrade(options: &options::Upgrade) -> anyhow::Result<()> {
    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => name,
        InstanceName::Cloud { .. } => todo!(),
    };
    let wsl = try_get_wsl()?;
    wsl.edgedb()
        .arg("instance")
        .arg("upgrade")
        .args(options)
        .run()?;
    // credentials might be updated on upgrade if we change format somehow
    wsl.copy_out(credentials_linux(name), credentials::path(name)?)?;
    Ok(())
}

pub fn revert(options: &options::Revert) -> anyhow::Result<()> {
    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => name,
        InstanceName::Cloud { .. } => todo!(),
    };
    let wsl = try_get_wsl()?;
    wsl.edgedb()
        .arg("instance")
        .arg("revert")
        .args(options)
        .run()?;
    // credentials might be updated on upgrade if we change format somehow
    wsl.copy_out(credentials_linux(name), credentials::path(name)?)?;
    Ok(())
}

pub fn read_jose_keys(name: &str) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let wsl = try_get_wsl()?;

    let data_dir = if name == "_localdev" {
        match env::var("EDGEDB_SERVER_DEV_DIR") {
            Ok(path) => if path.ends_with("/") {
                path
            } else {
                path + "/"
            },
            Err(_) => "/home/edgedb/.local/share/edgedb/_localdev/".into(),
        }
    } else {
        format!("/home/edgedb/.local/share/edgedb/data/{}/", name)
    };

    if !wsl.check_path_exist(&data_dir) {
        anyhow::bail!(NonLocalInstance);
    }
    Ok((
        wsl.read_text_file(data_dir.clone() + "edbjwskeys.pem")?.into_bytes(),
        wsl.read_text_file(data_dir + "edbjwekeys.pem")?.into_bytes(),
    ))
}

pub fn get_instance_info(name: &str) -> anyhow::Result<String> {
    let wsl = try_get_wsl()?;
    wsl.read_text_file(format!(
        "/home/edgedb/.local/share/edgedb/data/{}/instance_info.json",
        name))
}

pub fn is_in_wsl() -> bool {
    *IS_IN_WSL
}
