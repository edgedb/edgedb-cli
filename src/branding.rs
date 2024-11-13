#![allow(unused)]

/// The product name.
pub const BRANDING: &str = if cfg!(feature = "gel") {
    "Gel"
} else {
    "EdgeDB"
};
/// The CLI name.
pub const BRANDING_CLI: &str = const_format::concatcp!(BRANDING, " CLI");
/// The cloud name.
pub const BRANDING_CLOUD: &str = const_format::concatcp!(BRANDING, " Cloud");
/// The CLI command name.
pub const BRANDING_CLI_CMD: &str = if cfg!(feature = "gel") {
    "gel"
} else {
    "edgedb"
};
/// The CLI command name for the alternative executable.
pub const BRANDING_CLI_CMD_ALT: &str = if cfg!(feature = "gel") {
    "edgedb"
} else {
    "gel"
};
/// The executable file name for the CLI.
pub const BRANDING_CLI_CMD_FILE: &str = if cfg!(windows) {
    const_format::concatcp!(BRANDING_CLI_CMD, ".exe")
} else {
    BRANDING_CLI_CMD
};
/// The executable file name for the CLI alternative.
pub const BRANDING_CLI_CMD_ALT_FILE: &str = if cfg!(windows) {
    const_format::concatcp!(BRANDING_CLI_CMD_ALT, ".exe")
} else {
    BRANDING_CLI_CMD_ALT
};
/// The WSL distribution name.
pub const BRANDING_WSL: &str = "EdgeDB.WSL.1";
/// The display name for the configuration file.
pub const CONFIG_FILE_DISPLAY_NAME: &str = "`gel.toml` (or `edgedb.toml`)";
