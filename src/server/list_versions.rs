use crate::server::options::ListVersions;
use crate::server::detect::Detect;


pub fn list_versions(options: &ListVersions) -> Result<(), anyhow::Error> {
    let detect = Detect::current_os();
    let mut installed = Vec::new();
    for meth in detect.get_available_methods() {
        installed.extend(detect.get_installed(meth));
    }

    Ok(())
}
