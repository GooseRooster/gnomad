use std::process::Stdio;
use std::time::Duration;

/// Run a user-configured hook command via `sh -c`, injecting `envs`.
/// No-ops silently if `hook_cmd` is None or blank. Never returns an error —
/// failures (spawn error, non-zero exit, timeout) are only logged, matching
/// gnomad's convention that non-critical side channels never fail the
/// main pipeline.
pub async fn run(hook_cmd: Option<&str>, envs: &[(&str, String)]) {
    let Some(cmd) = hook_cmd else { return };
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return;
    }

    tracing::debug!("running hook: {cmd}");

    let mut command = tokio::process::Command::new("sh");
    command
        .args(["-c", cmd])
        .envs(envs.iter().map(|(k, v)| (*k, v.as_str())))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("hook failed to spawn: {e:#} (cmd: {cmd})");
            return;
        }
    };

    match tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await {
        Ok(Ok(output)) if output.status.success() => {
            tracing::debug!("hook completed successfully");
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("hook exited with {} (cmd: {cmd}): {}", output.status, stderr.trim());
        }
        Ok(Err(e)) => tracing::warn!("hook wait failed: {e:#} (cmd: {cmd})"),
        Err(_elapsed) => tracing::warn!("hook timed out after 30s (cmd: {cmd})"),
    }
}
