use crate::cloud::ops::get_versions;
use crate::cloud::client::CloudClient;
use crate::portable::ver;
use crate::portable::repository::{Channel, Query};


pub fn get_version(query: &Query, client: &CloudClient) -> anyhow::Result<ver::Specific> {
    let mut versions = get_versions(client)?;

    if let Some(v) = &query.version {
        versions.retain(
            |cand| v.matches_specific(
                &cand.version.parse::<ver::Specific>().unwrap()));
    }

    match query.channel {
        Channel::Stable => {
            versions.retain(
                |cand| {
                    let v = &cand.version.parse::<ver::Specific>().unwrap();
                    v.is_stable()
                }
            );
        },
        Channel::Testing => {
            versions.retain(
                |cand| {
                    let v = &cand.version.parse::<ver::Specific>().unwrap();
                    v.is_testing() || v.is_stable()
                }
            );
        },
        Channel::Nightly => {
            versions.retain(
                |cand| cand.version.parse::<ver::Specific>().unwrap().is_nightly()
            );
        },
    }

    if versions.is_empty() {
        anyhow::bail!(
            "no EdgeDB versions matching '{}' supported by EdgeDB Cloud",
            query.display(),
        );
    }

    versions.sort_by_cached_key(|k| k.version.parse::<ver::Specific>().unwrap());

    Ok(versions.last().unwrap().version.parse::<ver::Specific>().unwrap())
}
