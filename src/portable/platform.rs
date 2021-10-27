pub fn get_name() -> anyhow::Result<&'static str> {
    if cfg!(target_arch="x86_64") {
        if cfg!(target_os="windows") {
            return Ok("win-x86_64");
        } else if cfg!(target_os="macos") {
            return Ok("macos-x86_64");
        } else if cfg!(target_os="linux") {
            return Ok("linux-x86_64");
        } else {
            anyhow::bail!("unsupported OS on aarch64");
        }
    } else if cfg!(target_arch="aarch64") {
        if cfg!(target_os="macos") {
            return Ok("macos-aarch64");
        } else {
            anyhow::bail!("unsupported OS on aarch64")
        }
    } else {
        anyhow::bail!("unsupported architecture");
    }
}
