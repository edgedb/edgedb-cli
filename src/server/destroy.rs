use crate::server::detect;
use crate::server::errors::InstanceNotFound;
use crate::server::options::Destroy;
use crate::commands;


pub fn destroy(options: &Destroy) -> anyhow::Result<()> {
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;
    let mut errors = Vec::new();
    for meth in methods.values() {
        match meth.destroy(options) {
            Ok(()) => {}
            Err(e) if e.is::<InstanceNotFound>() => {
                errors.push((meth.name(), e));
            }
            Err(e) => Err(e)?,
        }
    }
    if errors.len() == methods.len() {
        eprintln!("No instances found:");
        for (meth, err) in errors {
            eprintln!("  * {}: {:#}", meth.title(), err);
        }
        Err(commands::ExitCode::new(1).into())
    } else {
        Ok(())
    }
}
