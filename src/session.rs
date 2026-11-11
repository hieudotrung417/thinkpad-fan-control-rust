use std::{
    env,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{mode::FanMode, HELPER_PATH, PKEXEC_PATH};

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("{0}")]
    Start(#[from] std::io::Error),
    #[error("Authorization was cancelled or denied.")]
    AuthorizationDenied,
    #[error("The privileged helper did not start.{0}")]
    NotReady(String),
    #[error("The privileged helper returned invalid data.")]
    InvalidData,
    #[error("The privileged helper session ended.")]
    Ended,
    #[error("{0}")]
    Helper(String),
}

#[derive(Serialize)]
struct Request {
    mode: FanMode,
}

#[derive(Deserialize)]
struct ReadyResponse {
    ready: bool,
}

#[derive(Deserialize)]
struct ModeResponse {
    ok: bool,
    message: Option<String>,
    error: Option<String>,
}

struct RunningHelper {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

/// One authenticated helper process, reused for this application session.
#[derive(Default)]
pub struct PrivilegedSession {
    helper: Option<RunningHelper>,
}

impl PrivilegedSession {
    pub fn ready(&mut self) -> bool {
        self.helper
            .as_mut()
            .is_some_and(|helper| helper.child.try_wait().ok().flatten().is_none())
    }

    pub fn open(&mut self) -> Result<(), SessionError> {
        if self.ready() {
            return Ok(());
        }
        self.close();
        let command = privileged_helper_command(&["serve"]);
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let input = child.stdin.take().ok_or(SessionError::Ended)?;
        let mut output = BufReader::new(child.stdout.take().ok_or(SessionError::Ended)?);
        let mut ready_line = String::new();
        if output.read_line(&mut ready_line)? == 0 {
            let status = child.wait()?;
            if matches!(status.code(), Some(126 | 127)) {
                return Err(SessionError::AuthorizationDenied);
            }
            let mut detail = String::new();
            if let Some(stderr) = child.stderr.take() {
                let _ = BufReader::new(stderr).read_line(&mut detail);
            }
            let suffix = if detail.trim().is_empty() {
                String::new()
            } else {
                format!(" {}", detail.trim())
            };
            return Err(SessionError::NotReady(suffix));
        }
        let ready: ReadyResponse =
            serde_json::from_str(&ready_line).map_err(|_| SessionError::InvalidData)?;
        if !ready.ready {
            let _ = child.kill();
            return Err(SessionError::InvalidData);
        }
        self.helper = Some(RunningHelper {
            child,
            input,
            output,
        });
        Ok(())
    }

    pub fn set_mode(&mut self, mode: FanMode) -> Result<String, SessionError> {
        self.open()?;
        let helper = self.helper.as_mut().ok_or(SessionError::Ended)?;
        serde_json::to_writer(&mut helper.input, &Request { mode })
            .map_err(|_| SessionError::Ended)?;
        helper
            .input
            .write_all(b"\n")
            .map_err(|_| SessionError::Ended)?;
        helper.input.flush().map_err(|_| SessionError::Ended)?;
        let mut response_line = String::new();
        if helper
            .output
            .read_line(&mut response_line)
            .map_err(|_| SessionError::Ended)?
            == 0
        {
            self.close();
            return Err(SessionError::Ended);
        }
        let response: ModeResponse =
            serde_json::from_str(&response_line).map_err(|_| SessionError::InvalidData)?;
        if response.ok {
            Ok(response
                .message
                .unwrap_or_else(|| "Fan mode changed.".to_owned()))
        } else {
            Err(SessionError::Helper(
                response
                    .error
                    .unwrap_or_else(|| "The helper failed.".to_owned()),
            ))
        }
    }

    pub fn close(&mut self) {
        let Some(mut helper) = self.helper.take() else {
            return;
        };
        drop(helper.input);
        if helper.child.try_wait().ok().flatten().is_none() {
            let _ = helper.child.kill();
        }
        let _ = helper.child.wait();
    }
}

impl Drop for PrivilegedSession {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn privileged_helper_command(arguments: &[&str]) -> Vec<String> {
    let mut command = vec![PKEXEC_PATH.to_owned()];
    if let Some(appimage) = env::var_os("APPIMAGE") {
        command.push(
            absolute_path(Path::new(&appimage))
                .to_string_lossy()
                .into_owned(),
        );
        command.push("--fan-helper".to_owned());
    } else {
        command.push(HELPER_PATH.to_owned());
    }
    command.extend(arguments.iter().map(|argument| (*argument).to_owned()));
    command
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        env::current_dir().unwrap_or_default().join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appimage_uses_itself_as_the_helper() {
        let old = env::var_os("APPIMAGE");
        env::set_var("APPIMAGE", "/tmp/ThinkPad Fan Control.AppImage");
        assert_eq!(
            privileged_helper_command(&["serve"]),
            [
                "/usr/bin/pkexec",
                "/tmp/ThinkPad Fan Control.AppImage",
                "--fan-helper",
                "serve"
            ]
        );
        match old {
            Some(value) => env::set_var("APPIMAGE", value),
            None => env::remove_var("APPIMAGE"),
        }
    }
}
