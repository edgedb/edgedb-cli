use crate::cloud::auth;


pub fn create(
    _cmd: &crate::portable::options::Create,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    println!("cloud create: {:?}", auth::get_access_token(&opts.cloud_options)?);
    Ok(())
}

pub fn link(
    _cmd: &crate::portable::options::Link,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    println!("cloud link: {:?}", auth::get_access_token(&opts.cloud_options)?);
    Ok(())
}
