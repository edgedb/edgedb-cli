use std::fmt;
use std::str::FromStr;

use edgedb_cli_derive::IntoArgs;

use crate::branding::BRANDING_CLOUD;
use crate::cloud::ops::CloudTier;
use crate::commands::ExitCode;
use crate::portable::local::{
    is_valid_cloud_instance_name, is_valid_cloud_org_name, is_valid_local_instance_name,
};
use crate::print::{err_marker, msg};
use crate::process::{self, IntoArg};

const DOMAIN_LABEL_MAX_LENGTH: usize = 63;
const CLOUD_INSTANCE_NAME_MAX_LENGTH: usize = DOMAIN_LABEL_MAX_LENGTH - 2 + 1; // "--" -> "/"

#[derive(Clone, Debug)]
pub enum InstanceName {
    Local(String),
    Cloud { org_slug: String, name: String },
}

impl From<edgedb_tokio::InstanceName> for InstanceName {
    fn from(x: edgedb_tokio::InstanceName) -> Self {
        match x {
            edgedb_tokio::InstanceName::Local(s) => InstanceName::Local(s),
            edgedb_tokio::InstanceName::Cloud { org_slug, name } => {
                InstanceName::Cloud { org_slug, name }
            }
        }
    }
}

impl fmt::Display for InstanceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstanceName::Local(name) => name.fmt(f),
            InstanceName::Cloud { org_slug, name } => write!(f, "{org_slug}/{name}"),
        }
    }
}

impl FromStr for InstanceName {
    type Err = anyhow::Error;
    fn from_str(name: &str) -> anyhow::Result<InstanceName> {
        if let Some((org_slug, instance_name)) = name.split_once('/') {
            if !is_valid_cloud_instance_name(instance_name) {
                anyhow::bail!(
                    "instance name \"{}\" must be a valid identifier, \
                     regex: ^[a-zA-Z0-9]+(-[a-zA-Z0-9]+)*$",
                    instance_name,
                );
            }
            if !is_valid_cloud_org_name(org_slug) {
                anyhow::bail!(
                    "org name \"{}\" must be a valid identifier, \
                     regex: ^-?[a-zA-Z0-9_]+(-[a-zA-Z0-9]+)*$",
                    org_slug,
                );
            }
            if name.len() > CLOUD_INSTANCE_NAME_MAX_LENGTH {
                anyhow::bail!(
                    "invalid {BRANDING_CLOUD} instance name \"{}\": \
                    length cannot exceed {} characters",
                    name,
                    CLOUD_INSTANCE_NAME_MAX_LENGTH,
                );
            }
            Ok(InstanceName::Cloud {
                org_slug: org_slug.into(),
                name: instance_name.into(),
            })
        } else {
            if !is_valid_local_instance_name(name) {
                anyhow::bail!(
                    "instance name must be a valid identifier, \
                     regex: ^[a-zA-Z_0-9]+(-[a-zA-Z_0-9]+)*$ or \
                     {BRANDING_CLOUD} instance name ORG/INST."
                );
            }
            Ok(InstanceName::Local(name.into()))
        }
    }
}

impl IntoArg for &InstanceName {
    fn add_arg(self, process: &mut process::Native) {
        process.arg(self.to_string());
    }
}

pub fn instance_arg(named: &Option<InstanceName>) -> anyhow::Result<InstanceName> {
    if let Some(name) = named {
        return Ok(name.clone());
    }

    {
        // infer instance from current project
        let bld = edgedb_tokio::Builder::new();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let config = rt.block_on(bld.build_env())?;

        let instance = config.instance_name().cloned();

        if let Some(instance) = instance {
            return Ok(instance.into());
        }
    };

    msg!(
        "{} Instance name argument is required, use '-I name'",
        err_marker()
    );
    Err(ExitCode::new(2).into())
}

#[derive(clap::Args, IntoArgs, Debug, Clone)]
pub struct CloudInstanceParams {
    /// The region in which to create the instance (for cloud instances).
    #[arg(long)]
    pub region: Option<String>,

    #[command(flatten)]
    pub billables: CloudInstanceBillables,
}

#[derive(clap::Args, IntoArgs, Debug, Clone)]
pub struct CloudInstanceBillables {
    /// Cloud instance subscription tier.
    #[arg(long, value_name = "tier")]
    #[arg(value_enum)]
    pub tier: Option<CloudTier>,

    /// The size of compute to be allocated for the Cloud instance in
    /// Compute Units.
    #[arg(long, value_name="number", value_parser=billable_unit)]
    pub compute_size: Option<String>,

    /// The size of storage to be allocated for the Cloud instance in
    /// Gigabytes.
    #[arg(long, value_name="GiB", value_parser=billable_unit)]
    pub storage_size: Option<String>,
}

fn billable_unit(s: &str) -> Result<String, String> {
    let (numerator, denominator) = match s.split_once('/') {
        Some(v) => v,
        None => (s, "1"),
    };

    let n: u64 = numerator
        .parse()
        .map_err(|_| format!("`{s}` is not a positive number or valid fraction"))?;

    let d: u64 = denominator
        .parse()
        .map_err(|_| format!("`{s}` is not a positive number or valid fraction"))?;

    if n == 0 || d == 0 {
        Err(String::from(
            "`{s}` is not a positive number or valid fraction",
        ))
    } else {
        Ok(s.to_string())
    }
}
