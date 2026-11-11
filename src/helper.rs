use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::{control::FanController, mode::FanMode};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    mode: FanMode,
}

#[derive(Debug, Serialize)]
struct Response<'a> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

pub fn serve<R, W, F, E>(input: R, mut output: W, mut set_mode: F) -> io::Result<()>
where
    R: BufRead,
    W: Write,
    F: FnMut(FanMode) -> Result<String, E>,
    E: std::fmt::Display,
{
    writeln!(output, "{{\"ready\":true}}")?;
    output.flush()?;
    for line in input.lines() {
        let line = line?;
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => match set_mode(request.mode) {
                Ok(message) => serde_json::to_string(&Response {
                    ok: true,
                    message: Some(&message),
                    error: None,
                }),
                Err(error) => {
                    let error = error.to_string();
                    serde_json::to_string(&Response {
                        ok: false,
                        message: None,
                        error: Some(&error),
                    })
                }
            },
            Err(_) => serde_json::to_string(&Response {
                ok: false,
                message: None,
                error: Some("Invalid helper request."),
            }),
        }
        .expect("serializing a helper response cannot fail");
        writeln!(output, "{response}")?;
        output.flush()?;
    }
    Ok(())
}

pub fn main(arguments: &[String]) -> i32 {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("This helper must be run through pkexec.");
        return 1;
    }
    let controller = FanController::default();
    match arguments {
        [command] if command == "serve" => {
            let stdin = io::stdin();
            let stdout = io::stdout();
            match serve(stdin.lock(), stdout.lock(), |mode| {
                controller.set_mode(mode)
            }) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("Helper session failed: {error}");
                    1
                }
            }
        }
        [command, mode] if command == "set" => {
            let Ok(mode) = mode.parse::<FanMode>() else {
                eprintln!("Invalid fan mode.");
                return 1;
            };
            match controller.set_mode(mode) {
                Ok(message) => {
                    println!("{}", serde_json::json!({"ok": true, "message": message}));
                    0
                }
                Err(error) => {
                    eprintln!("{error}");
                    1
                }
            }
        }
        _ => {
            eprintln!("Usage: fanspeed-helper {{serve|set {{auto|1..7|full-speed}}}}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn validates_every_server_request() {
        let input =
            Cursor::new("{\"mode\":\"4\"}\n{\"mode\":\"0\"}\n{\"mode\":\"5\",\"extra\":1}\n");
        let mut output = Vec::new();
        serve(input, &mut output, |mode| {
            Ok::<_, &'static str>(format!("Level {mode}"))
        })
        .unwrap();
        let lines: Vec<serde_json::Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines[0], serde_json::json!({"ready": true}));
        assert_eq!(
            lines[1],
            serde_json::json!({"ok": true, "message": "Level 4"})
        );
        assert_eq!(
            lines[2],
            serde_json::json!({"ok": false, "error": "Invalid helper request."})
        );
        assert_eq!(
            lines[3],
            serde_json::json!({"ok": false, "error": "Invalid helper request."})
        );
    }
}
