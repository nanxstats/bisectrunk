use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};

#[derive(Clone, Debug)]
pub(crate) struct HookContext<'a> {
    pub(crate) commit: &'a str,
    pub(crate) worktree: &'a Path,
    pub(crate) env_dir: &'a Path,
    pub(crate) out_dir: &'a Path,
    pub(crate) project: &'a Path,
    pub(crate) run_dir: &'a Path,
    pub(crate) job: usize,
    pub(crate) extra_env: &'a BTreeMap<String, String>,
    pub(crate) pin_envs: Option<&'a str>,
    pub(crate) baseline: Option<&'a Path>,
    pub(crate) candidate: Option<&'a Path>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HookResult {
    pub(crate) code: i32,
    pub(crate) timed_out: bool,
}

pub(crate) fn execute(
    command: &str,
    shell_override: Option<&Path>,
    cwd: &Path,
    log_path: &Path,
    context: &HookContext<'_>,
    timeout: Option<Duration>,
) -> Result<HookResult> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create hook log directory {}", parent.display()))?;
    }
    let stdout = File::create(log_path)
        .with_context(|| format!("create hook log {}", log_path.display()))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("duplicate hook log handle {}", log_path.display()))?;
    let mut process = shell_command(shell_override, command);
    process
        .current_dir(cwd)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    set_contract_env(&mut process, context);
    let mut child = process
        .spawn()
        .with_context(|| format!("start hook {command:?} for commit {}", context.commit))?;
    let (status, timed_out) = wait(&mut child, timeout)
        .with_context(|| format!("wait for hook {command:?} at commit {}", context.commit))?;
    Ok(HookResult {
        code: exit_code(status),
        timed_out,
    })
}

fn shell_command(shell_override: Option<&Path>, command: &str) -> Command {
    if let Some(shell) = shell_override {
        let mut process = Command::new(shell);
        #[cfg(windows)]
        process.arg("/C");
        #[cfg(not(windows))]
        process.arg("-c");
        process.arg(command);
        return process;
    }
    #[cfg(windows)]
    let mut process = {
        let mut value = Command::new("cmd");
        value.arg("/C");
        value
    };
    #[cfg(not(windows))]
    let mut process = {
        let mut value = Command::new("sh");
        value.arg("-c");
        value
    };
    process.arg(command);
    process
}

fn set_contract_env(process: &mut Command, context: &HookContext<'_>) {
    process
        .env("BISECTRUNK_COMMIT", context.commit)
        .env(
            "BISECTRUNK_COMMIT_SHORT",
            crate::util::short_sha(context.commit),
        )
        .env("BISECTRUNK_WORKTREE", context.worktree)
        .env("BISECTRUNK_ENV", context.env_dir)
        .env("BISECTRUNK_OUT", context.out_dir)
        .env("BISECTRUNK_PROJECT", context.project)
        .env("BISECTRUNK_JOB", context.job.to_string())
        .env("BISECTRUNK_RUN_DIR", context.run_dir)
        .envs(context.extra_env);
    if let Some(pin_envs) = context.pin_envs {
        process.env("BISECTRUNK_PIN_ENVS", pin_envs);
    }
    if let Some(baseline) = context.baseline {
        process.env("BISECTRUNK_BASELINE", baseline);
    }
    if let Some(candidate) = context.candidate {
        process.env("BISECTRUNK_CANDIDATE", candidate);
    }
}

fn wait(child: &mut std::process::Child, timeout: Option<Duration>) -> Result<(ExitStatus, bool)> {
    let Some(timeout) = timeout else {
        return child
            .wait()
            .map(|status| (status, false))
            .map_err(Into::into);
    };
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().context("poll hook process")? {
            return Ok((status, false));
        }
        if started.elapsed() >= timeout {
            child.kill().context("kill timed-out hook process")?;
            let status = child.wait().context("reap timed-out hook process")?;
            return Ok((status, true));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|signal| 128 + signal).unwrap_or(128)
    }
    #[cfg(not(unix))]
    {
        128
    }
}

pub(crate) fn parse_timeout(value: Option<&str>) -> Result<Option<Duration>> {
    value
        .map(|raw| {
            humantime::parse_duration(raw)
                .map_err(|error| anyhow!(error))
                .with_context(|| format!("parse timeout duration {raw:?}"))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_human_durations() {
        assert_eq!(
            parse_timeout(Some("2m")).expect("valid").unwrap(),
            Duration::from_secs(120)
        );
        assert!(parse_timeout(Some("tomorrow")).is_err());
        assert!(parse_timeout(None).expect("none is valid").is_none());
    }
}
