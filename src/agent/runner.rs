use std::{fs, path::PathBuf, time::Duration};

use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    process::ChildStdout,
    sync::watch,
    task::JoinHandle,
};

use super::{
    codex::{CodexExecutable, Doctor, command},
    policy::{
        MAX_JSONL_LINE_BYTES, MAX_STDOUT_BYTES, sanitize_draft, scratch_directory, validate_prompt,
        validate_workspace,
    },
};

const RUN_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentDraft {
    pub text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunFailure {
    Unavailable,
    Busy,
    TimedOut,
    Cancelled,
    InvalidOutput,
    Failed,
}

impl RunFailure {
    pub const fn message(self) -> &'static str {
        match self {
            Self::Unavailable => "local Codex is unavailable",
            Self::Busy => "a local assistant draft is already running",
            Self::TimedOut => "local Codex timed out",
            Self::Cancelled => "local Codex draft cancelled",
            Self::InvalidOutput => "local Codex returned unusable output",
            Self::Failed => "local Codex failed",
        }
    }
}

pub struct AgentRun {
    task: JoinHandle<Result<AgentDraft, RunFailure>>,
    cancel: watch::Sender<bool>,
}

impl AgentRun {
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    pub async fn finish(self) -> Result<AgentDraft, RunFailure> {
        let Self { task, cancel } = self;
        let result = task.await.unwrap_or(Err(RunFailure::Cancelled));
        drop(cancel);
        result
    }

    pub fn cancel(&self) {
        let _ = self.cancel.send(true);
    }
}

/// Starts one isolated local process. The caller owns the only active handle and
/// must cancel it before replacing or shutting down the surrounding session.
pub fn start(
    executable: CodexExecutable,
    prompt: String,
    data_dir: PathBuf,
    configured_workdir: Option<PathBuf>,
) -> Result<AgentRun, RunFailure> {
    validate_prompt(&prompt).map_err(|_| RunFailure::InvalidOutput)?;
    let configured_workdir = configured_workdir
        .map(|path| validate_workspace(&path))
        .transpose()
        .map_err(|_| RunFailure::Failed)?;
    let scratch = configured_workdir
        .is_none()
        .then(|| scratch_directory(&data_dir).map(ScratchDirectory))
        .transpose()
        .map_err(|_| RunFailure::Failed)?;
    let cwd = configured_workdir
        .or_else(|| scratch.as_ref().map(|scratch| scratch.0.clone()))
        .ok_or(RunFailure::Failed)?;
    let (cancel, cancelled) = watch::channel(false);
    let task = tokio::spawn(async move {
        let _scratch = scratch;
        let mut cancelled = cancelled;
        let doctor = tokio::select! {
            doctor = executable.doctor() => doctor,
            changed = cancelled.changed() => {
                let _ = changed;
                return Err(RunFailure::Cancelled);
            }
        };
        match doctor {
            Doctor::Ready => run(executable, prompt, cwd, cancelled).await,
            Doctor::Unavailable => Err(RunFailure::Unavailable),
            Doctor::Unsupported => Err(RunFailure::Unavailable),
        }
    });
    Ok(AgentRun { task, cancel })
}

struct ScratchDirectory(PathBuf);

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn run(
    executable: CodexExecutable,
    prompt: String,
    cwd: PathBuf,
    mut cancelled: watch::Receiver<bool>,
) -> Result<AgentDraft, RunFailure> {
    let mut child = command(&executable, &cwd)
        .spawn()
        .map_err(|_| RunFailure::Unavailable)?;
    let mut stdin = child.stdin.take().ok_or(RunFailure::Unavailable)?;
    let stdout = child.stdout.take().ok_or(RunFailure::Unavailable)?;
    let stderr = child.stderr.take().ok_or(RunFailure::Unavailable)?;
    let prompt_writer = tokio::spawn(async move {
        stdin.write_all(prompt.as_bytes()).await?;
        stdin.shutdown().await
    });
    let stderr_drain = tokio::spawn(async move {
        let mut stderr = stderr;
        let mut sink = tokio::io::sink();
        tokio::io::copy(&mut stderr, &mut sink).await
    });
    let output = tokio::spawn(read_jsonl(stdout));
    let status = tokio::select! {
        status = child.wait() => status.map_err(|_| RunFailure::Failed)?,
        _ = tokio::time::sleep(RUN_TIMEOUT) => {
            prompt_writer.abort();
            output.abort();
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stderr_drain.await;
            return Err(RunFailure::TimedOut);
        }
        changed = cancelled.changed() => {
            let _ = changed;
            prompt_writer.abort();
            output.abort();
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stderr_drain.await;
            return Err(RunFailure::Cancelled);
        }
    };
    let _ = prompt_writer.await;
    let _ = stderr_drain.await;
    let draft = output
        .await
        .map_err(|_| RunFailure::Cancelled)?
        .map_err(|_| RunFailure::InvalidOutput)?;
    if !status.success() {
        return Err(RunFailure::Failed);
    }
    draft
        .map(|text| AgentDraft { text })
        .ok_or(RunFailure::InvalidOutput)
}

async fn read_jsonl(mut stdout: ChildStdout) -> Result<Option<String>, ()> {
    let mut total = 0_usize;
    let mut line = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut final_draft = None;
    loop {
        let count = stdout.read(&mut chunk).await.map_err(|_| ())?;
        if count == 0 {
            if !line.is_empty() {
                final_draft = parse_line(&line, final_draft)?;
            }
            return Ok(final_draft);
        }
        total = total.saturating_add(count);
        if total > MAX_STDOUT_BYTES {
            return Err(());
        }
        for byte in &chunk[..count] {
            if *byte == b'\n' {
                final_draft = parse_line(&line, final_draft)?;
                line.clear();
            } else {
                line.push(*byte);
                if line.len() > MAX_JSONL_LINE_BYTES {
                    return Err(());
                }
            }
        }
    }
}

fn parse_line(line: &[u8], previous: Option<String>) -> Result<Option<String>, ()> {
    if line.is_empty() {
        return Err(());
    }
    let event = serde_json::from_slice::<JsonlEvent>(line).map_err(|_| ())?;
    if event.kind != "item.completed" {
        return Ok(previous);
    }
    let item = event.item.ok_or(())?;
    if item.kind != "agent_message" {
        return Ok(previous);
    }
    let text = item.text.ok_or(())?;
    sanitize_draft(&text).map(Some).map_err(|_| ())
}

#[derive(Deserialize)]
struct JsonlEvent {
    #[serde(rename = "type")]
    kind: String,
    item: Option<JsonlItem>,
}

#[derive(Deserialize)]
struct JsonlItem {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{RunFailure, parse_line, start};
    use crate::agent::codex::CodexExecutable;

    #[test]
    fn only_completed_agent_messages_become_drafts() {
        let ignored = br#"{"type":"item.completed","item":{"type":"reasoning","text":"ignore"}}"#;
        assert_eq!(parse_line(ignored, None).unwrap(), None);
        let first = br#"{"type":"item.completed","item":{"type":"agent_message","text":"first"}}"#;
        let second = br#"{"type":"item.completed","item":{"type":"agent_message","text":"final"}}"#;
        let result = parse_line(first, None).unwrap();
        assert_eq!(
            parse_line(second, result).unwrap().as_deref(),
            Some("final")
        );
        assert!(parse_line(b"not json", None).is_err());
        assert_eq!(RunFailure::TimedOut.message(), "local Codex timed out");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_codex_receives_only_safe_flags_stdin_and_minimal_environment() {
        use std::{fs, os::unix::fs::PermissionsExt};

        let temporary = tempfile::TempDir::new().unwrap();
        let executable = temporary.path().join("codex");
        fs::write(
            &executable,
            "#!/bin/sh\nif [ \"$2\" = \"--help\" ]; then\n  printf '%s\\n' '--json --ephemeral --ignore-user-config --ignore-rules --sandbox read-only --skip-git-repo-check'\n  exit 0\nfi\n[ \"$1\" = exec ] || exit 11\n[ -z \"${OPENAI_API_KEY:-}\" ] || exit 12\ncase \" $* \" in *' --full-auto '*|*' --dangerously-bypass-approvals-and-sandbox '*) exit 13;; esac\ncat >/dev/null\nprintf '%s\\n' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"safe draft\\u001b[2J\"}}'\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let executable = CodexExecutable::from_test_path(executable.canonicalize().unwrap());
        let data_dir = temporary.path().join("data");
        fs::create_dir(&data_dir).unwrap();
        let draft = start(executable, "quoted context".into(), data_dir.clone(), None)
            .unwrap()
            .finish()
            .await
            .unwrap();
        assert_eq!(draft.text, "safe draft�[2J");
        assert!(
            fs::read_dir(data_dir.join("agent-scratch"))
                .unwrap()
                .next()
                .is_none()
        );
    }
}
