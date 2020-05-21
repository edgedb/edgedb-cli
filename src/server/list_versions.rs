use crate::server::options::ListVersions;


pub fn list_versions(_options: &ListVersions) -> Result<(), anyhow::Error> {
    todo!();
    /*
    let detect = Detect::current_os();
    let mut installed = Vec::new();
    for meth in detect.get_available_methods() {
        installed.extend(detect.get_installed(meth));
    }

    Ok(())
    */
}
