use anyhow::{Context, Result};
use tokio::process::Command;

pub async fn apply_scheme(slug: &str) -> Result<()> {
    let output = Command::new("tinty")
        .args(["apply", slug])
        .output()
        .await
        .context("spawning tinty")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tinty apply failed: {stderr}");
    }
    Ok(())
}
