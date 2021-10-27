use crate::server::options::ListVersions;

use crate::portable::repository::{get_server_packages, Channel};
use crate::eecho;
use crate::print::Highlight;


pub fn list_versions(options: &ListVersions) -> Result<(), anyhow::Error> {
    if options.deprecated_install_methods {
        return crate::server::list_versions::list_versions(options);
    }
    if options.installed_only {
        todo!();
    } else {
        let mut packages = get_server_packages(Channel::Stable)?;
        packages.extend(get_server_packages(Channel::Nightly)?);
        // TODO(tailhook) get installed, print table
        println!("{:#?}", packages);
    }
    eecho!("Only portable packages shown here, \
        use `--deprecated-install-method` \
        to show docker and package installations.".fade());
    Ok(())
}
