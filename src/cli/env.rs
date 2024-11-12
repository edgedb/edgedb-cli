use edgedb_tokio::define_env;
use std::path::PathBuf;

define_env! {
    /// The password to use when connecting to the database.
    #[env(EDGEDB_PASSWORD, GEL_PASSWORD)]
    password: String,

    /// Path to the editor executable
    #[env(EDGEDB_EDITOR, GEL_EDITOR)]
    editor: String,

    /// Whether to install in Docker
    #[env(EDGEDB_INSTALL_IN_DOCKER, GEL_INSTALL_IN_DOCKER)]
    install_in_docker: InstallInDocker,

    /// Development server directory path
    #[env(EDGEDB_SERVER_DEV_DIR, GEL_SERVER_DEV_DIR)]
    server_dev_dir: PathBuf,

    /// Whether to run version check
    #[env(EDGEDB_RUN_VERSION_CHECK, GEL_RUN_VERSION_CHECK)]
    run_version_check: VersionCheck,

    /// Path to pager executable
    #[env(EDGEDB_PAGER, GEL_PAGER)]
    pager: String,

    /// Path to test binary executable
    #[env(EDGEDB_TEST_BIN_EXE, GEL_TEST_BIN_EXE)]
    test_bin_exe: PathBuf,

    /// Debug flag for analyze JSON output
    #[env(_EDGEDB_ANALYZE_DEBUG_JSON, _GEL_ANALYZE_DEBUG_JSON)]
    _analyze_debug_json: bool,

    /// Debug flag for analyze plan output
    #[env(_EDGEDB_ANALYZE_DEBUG_PLAN, _GEL_ANALYZE_DEBUG_PLAN)]
    _analyze_debug_plan: bool,

    /// Cloud profile name
    #[env(EDGEDB_CLOUD_PROFILE, GEL_CLOUD_PROFILE)]
    cloud_profile: String,

    /// Cloud secret key
    #[env(EDGEDB_CLOUD_SECRET_KEY, GEL_CLOUD_SECRET_KEY)]
    cloud_secret_key: String,

    /// Legacy secret key
    #[env(EDGEDB_SECRET_KEY, GEL_SECRET_KEY)]
    secret_key: String,

    /// Cloud API endpoint URL
    #[env(EDGEDB_CLOUD_API_ENDPOINT, GEL_CLOUD_API_ENDPOINT)]
    cloud_api_endpoint: String,

    /// Cloud certificates configuration
    #[env(_EDGEDB_CLOUD_CERTS, _GEL_CLOUD_CERTS)]
    _cloud_certs: String,

    /// WSL distro name
    #[env(_EDGEDB_WSL_DISTRO, _GEL_WSL_DISTRO)]
    _wsl_distro: String,

    /// Path to WSL Linux binary
    #[env(_EDGEDB_WSL_LINUX_BINARY, _GEL_WSL_LINUX_BINARY)]
    _wsl_linux_binary: PathBuf,

    /// Flag indicating Windows wrapper
    #[env(_EDGEDB_FROM_WINDOWS, _GEL_FROM_WINDOWS)]
    _from_windows: bool,

    /// Package repository root URL
    #[env(EDGEDB_PKG_ROOT, GEL_PKG_ROOT)]
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
