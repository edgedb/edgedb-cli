use color_print::cformat;

use crate::cloud;
use crate::portable::options::{Backup, InstanceName, ListBackups, Restore};
use crate::print::echo;
use crate::question;

pub fn list(cmd: &ListBackups, opts: &crate::options::Options) -> anyhow::Result<()> {
    match &cmd.instance {
        InstanceName::Local(_) => Err(opts.error(
            clap::error::ErrorKind::InvalidValue,
            cformat!("list-backups can only operate on Cloud instances."),
        ))?,
        InstanceName::Cloud {
            org_slug: org,
            name,
        } => list_cloud_backups_cmd(cmd, org, name, opts),
    }
}

fn list_cloud_backups_cmd(
    cmd: &ListBackups,
    org_slug: &str,
    name: &str,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    let client = cloud::client::CloudClient::new(&opts.cloud_options)?;
    client.ensure_authenticated()?;

    cloud::backups::list_cloud_instance_backups(&client, org_slug, name, cmd.json)?;

    Ok(())
}

pub fn backup(cmd: &Backup, opts: &crate::options::Options) -> anyhow::Result<()> {
    match &cmd.instance {
        InstanceName::Local(_) => Err(opts.error(
            clap::error::ErrorKind::InvalidValue,
            cformat!("Only Cloud instances can be backed up using this command."),
        ))?,
        InstanceName::Cloud {
            org_slug: org,
            name,
        } => backup_cloud_cmd(cmd, org, name, opts),
    }
}

fn backup_cloud_cmd(
    cmd: &Backup,
    org_slug: &str,
    name: &str,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    let client = cloud::client::CloudClient::new(&opts.cloud_options)?;
    client.ensure_authenticated()?;

    let inst_name = InstanceName::Cloud {
        org_slug: org_slug.to_string(),
        name: name.to_string(),
    };

    let prompt = format!(
        "Will create a backup for the \"{inst_name}\" Cloud instance:\
        \n\nContinue?",
    );

    if !cmd.non_interactive && !question::Confirm::new(prompt).ask()? {
        return Ok(());
    }

    let request = cloud::backups::CloudInstanceBackup {
        name: name.to_string(),
        org: org_slug.to_string(),
    };
    cloud::backups::backup_cloud_instance(&client, &request)?;

    echo!(
        "Successfully created a backup for EdgeDB Cloud instance",
        inst_name,
    );
    Ok(())
}

pub fn restore(cmd: &Restore, opts: &crate::options::Options) -> anyhow::Result<()> {
    match &cmd.instance {
        InstanceName::Local(_) => Err(opts.error(
            clap::error::ErrorKind::InvalidValue,
            cformat!("Only Cloud instances can be restored."),
        ))?,
        InstanceName::Cloud {
            org_slug: org,
            name,
        } => restore_cloud_cmd(cmd, org, name, opts),
    }
}

fn restore_cloud_cmd(
    cmd: &Restore,
    org_slug: &str,
    name: &str,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    let backup = &cmd.backup_spec;

    let client = cloud::client::CloudClient::new(&opts.cloud_options)?;
    client.ensure_authenticated()?;

    let inst_name = InstanceName::Cloud {
        org_slug: org_slug.to_string(),
        name: name.to_string(),
    };

    let source_inst = match &cmd.source_instance {
        Some(InstanceName::Local(_)) => Err(opts.error(
            clap::error::ErrorKind::InvalidValue,
            cformat!("--source-instance can only be a Cloud instance"),
        ))?,
        Some(InstanceName::Cloud { org_slug, name }) => {
            let inst = cloud::ops::find_cloud_instance_by_name(name, org_slug, &client)?
                .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
            Some(inst)
        }
        None => None,
    };

    let prompt = format!(
        "Will restore the \"{inst_name}\" Cloud instance from the specified backup:\
        \n\nContinue?",
    );

    if !cmd.non_interactive && !question::Confirm::new(prompt).ask()? {
        return Ok(());
    }

    let request = cloud::backups::CloudInstanceRestore {
        name: name.to_string(),
        org: org_slug.to_string(),
        backup_id: backup.backup_id.clone(),
        latest: backup.latest,
        source_instance_id: source_inst.and_then(|i| Some(i.id)),
    };
    cloud::backups::restore_cloud_instance(&client, &request)?;

    echo!(
        "EdgeDB Cloud instance",
        inst_name,
        "has been restored successfully."
    );
    echo!("To connect to the instance run:");
    echo!("  edgedb -I", inst_name);
    Ok(())
}
