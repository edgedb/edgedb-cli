use edgedb_tokio::define_env;
use std::path::PathBuf;

define_env! {
    /// Path to the editor executable
    #[env(GEL_EDITOR, EDGEDB_EDITOR)]
    editor: String,

    /// Whether to install in Docker
    #[env(GEL_INSTALL_IN_DOCKER, EDGEDB_INSTALL_IN_DOCKER)]
    install_in_docker: InstallInDocker,

    /// Development server directory path
    #[env(GEL_SERVER_DEV_DIR, EDGEDB_SERVER_DEV_DIR)]
    server_dev_dir: PathBuf,

    /// Whether to run version check
    #[env(GEL_RUN_VERSION_CHECK, EDGEDB_RUN_VERSION_CHECK)]
    run_version_check: VersionCheck,

    /// Path to pager executable
    #[env(GEL_PAGER, EDGEDB_PAGER)]
    pager: String,

    /// Debug flag for analyze JSON output
    #[env(_GEL_ANALYZE_DEBUG_JSON, _EDGEDB_ANALYZE_DEBUG_JSON)]
    _analyze_debug_json: bool,

    /// Debug flag for analyze plan output
    #[env(_GEL_ANALYZE_DEBUG_PLAN, _EDGEDB_ANALYZE_DEBUG_PLAN)]
    _analyze_debug_plan: bool,

    /// Cloud secret key
    #[env(GEL_CLOUD_SECRET_KEY, EDGEDB_CLOUD_SECRET_KEY)]
    cloud_secret_key: String,

    /// Cloud API endpoint URL
    #[env(GEL_CLOUD_API_ENDPOINT, EDGEDB_CLOUD_API_ENDPOINT)]
    cloud_api_endpoint: String,

    /// WSL distro name
    #[env(_GEL_WSL_DISTRO, _EDGEDB_WSL_DISTRO)]
    _wsl_distro: String,

    /// Path to WSL Linux binary
    #[env(_GEL_WSL_LINUX_BINARY, _EDGEDB_WSL_LINUX_BINARY)]
    _wsl_linux_binary: PathBuf,

    /// Flag indicating Windows wrapper
    #[env(_GEL_FROM_WINDOWS, _EDGEDB_FROM_WINDOWS)]
    _from_windows: bool,

    /// Package repository root URL
    #[env(GEL_PKG_ROOT, EDGEDB_PKG_ROOT)]
    pkg_root: String,

    /// System editor
    #[env(EDITOR)]
    system_editor: String,

    /// System pager
    #[env(PAGER)]
    system_pager: String,
}

#[derive(Debug)]
pub enum VersionCheck {
    Never,
    Cached,
    Default,
    Strict,
}

impl std::str::FromStr for VersionCheck {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "never" => Ok(Self::Never),
            "cached" => Ok(Self::Cached),
            "default" => Ok(Self::Default),
            "strict" => Ok(Self::Strict),
            _ => Err(format!("Invalid value: {}", s)),
        }
    }
}

#[derive(Debug)]
pub enum InstallInDocker {
    Forbid,
    Allow,
    Default,
}

impl std::str::FromStr for InstallInDocker {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "forbid" => Ok(Self::Forbid),
            "allow" => Ok(Self::Allow),
            "default" => Ok(Self::Default),
            _ => Err(format!("Invalid value: {}", s)),
        }
    }
}
