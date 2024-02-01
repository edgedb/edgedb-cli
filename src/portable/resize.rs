use color_print::cformat;

use crate::cloud;
use crate::portable::options::{Resize, InstanceName};
use crate::print::echo;
use crate::question;


pub fn resize(cmd: &Resize, opts: &crate::options::Options) -> anyhow::Result<()> {
    match &cmd.instance {
        InstanceName::Local(_) => {
            Err(opts.error(
                clap::error::ErrorKind::InvalidValue,
                cformat!("Only Cloud instances can be resized."),
            ))?
        },
        InstanceName::Cloud { org_slug: org, name } => {
            resize_cloud_cmd(cmd, org, name, opts)
        },
    }
}

fn resize_cloud_cmd(
    cmd: &Resize,
    org_slug: &str,
    name: &str,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    let billables = &cmd.billables;

    if billables.tier.is_none()
        && billables.compute_size.is_none()
        && billables.storage_size.is_none()
    {
        return Err(opts.error(
            clap::error::ErrorKind::MissingRequiredArgument,
            cformat!("Either <bold>--tier</bold>, <bold>--compute-size</bold>, \
            or <bold>--storage-size</bold> must be specified."),
        ))?;
    }

    if billables.compute_size.is_some() && billables.storage_size.is_some() {
        return Err(opts.error(
            clap::error::ErrorKind::MissingRequiredArgument,
            cformat!("<bold>--compute-size</bold>, \
            and <bold>--storage-size</bold> cannot be modified at the same time."),
        ))?;
    }

    let client = cloud::client::CloudClient::new(&opts.cloud_options)?;
    client.ensure_authenticated()?;

    let inst_name = InstanceName::Cloud {
        org_slug: org_slug.to_string(),
        name: name.to_string(),
    };

    let inst = cloud::ops::find_cloud_instance_by_name(name, org_slug, &client)?
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;

    let compute_size = billables.compute_size;
    let storage_size = billables.storage_size;
    let mut resources_display_vec: Vec<String> = vec![];

    if let Some(tier) = billables.tier {
        if tier == inst.tier && compute_size.is_none() && storage_size.is_none() {
            return Err(opts.error(
                clap::error::ErrorKind::InvalidValue,
                cformat!("Instance \"{org_slug}/{name}\" is already a {tier:?} \
                instance."),
            ))?;
        }

        if tier == cloud::ops::CloudTier::Free {
            if compute_size.is_some() {
                return Err(opts.error(
                    clap::error::ErrorKind::ArgumentConflict,
                    cformat!("The <bold>--compute-size</bold> option can \
                    only be specified for Pro instances."),
                ))?;
            }
            if storage_size.is_some() {
                return Err(opts.error(
                    clap::error::ErrorKind::ArgumentConflict,
                    cformat!("The <bold>--storage-size</bold> option can \
                    only be specified for Pro instances."),
                ))?;
            }
        }

        if tier != inst.tier {
            resources_display_vec.push(format!(
                "New Tier: {tier:?}",
            ));
        }
    }

    let mut req_resources: Vec<cloud::ops::CloudInstanceResourceRequest> = vec![];

    if let Some(compute_size) = compute_size {
        req_resources.push(
            cloud::ops::CloudInstanceResourceRequest{
                name: "compute".to_string(),
                value: compute_size,
            },
        );
        resources_display_vec.push(format!(
            "New Compute Size: {} compute unit{}",
            compute_size,
            if compute_size == 1 {""} else {"s"},
        ));
    }

    if let Some(storage_size) = storage_size {
        req_resources.push(
            cloud::ops::CloudInstanceResourceRequest{
                name: "storage".to_string(),
                value: storage_size,
            },
        );
        resources_display_vec.push(format!(
            "New Storage Size: {} gigabyte{}",
            storage_size,
            if storage_size == 1 {""} else {"s"},
        ));
    }

    let mut resources_display = resources_display_vec.join("\n");
    if resources_display != "" {
        resources_display = format!("\n{resources_display}");
    }

    let prompt = format!(
        "Will resize the \"{inst_name}\" Cloud instance as follows:\
        \n\
        {resources_display}\
        \n\nContinue?",
    );

    if !cmd.non_interactive && !question::Confirm::new(prompt).ask()? {
        return Ok(());
    }

    let request = cloud::ops::CloudInstanceResize {
        name: name.to_string(),
        org: org_slug.to_string(),
        requested_resources: Some(req_resources),
        tier: billables.tier,
    };
    cloud::ops::resize_cloud_instance(&client, &request)?;
    echo!(
        "EdgeDB Cloud instance",
        inst_name,
        "has been resized successfuly."
    );
    echo!("To connect to the instance run:");
    echo!("  edgedb -I", inst_name);
    return Ok(())
}
