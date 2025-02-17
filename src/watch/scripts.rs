use std::path;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::portable::windows;
use crate::print;
use crate::process;

use super::Target;
use super::{Context, ExecutionOrder, Matcher};

pub async fn execute(
    mut input: UnboundedReceiver<ExecutionOrder>,
    matcher: Arc<Matcher>,
    ctx: Arc<Context>,
) {
    let project_root = &ctx.project.location.root;

    while let Some(order) = ExecutionOrder::recv(&mut input).await {
        order.print(&matcher, ctx.as_ref());

        let Target::Script(script) = &matcher.target else {
            unreachable!()
        };

        let res = run_script(matcher.name(), &script, project_root).await;

        match res {
            Ok(status) => {
                if !status.success() {
                    print::error!("script exited with status {status}");
                }
            }
            Err(e) => {
                print::error!("{e}")
            }
        }
    }
}

pub async fn run_script(
    marker: &str,
    script: &str,
    current_dir: &path::Path,
) -> Result<std::process::ExitStatus, anyhow::Error> {
    let status = if !cfg!(windows) {
        let marker = marker.to_string();
        process::Native::new("", marker, "/bin/sh")
            .arg("-c")
            .arg(script)
            .current_dir(current_dir)
            .run_for_status()
            .await?
    } else {
        let wsl = windows::try_get_wsl()?;
        wsl.sh(current_dir)
            .arg("-c")
            .arg(script)
            .run_for_status()
            .await?
    };
    Ok(status)
}
