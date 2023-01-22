use crate::recording_pool::recording_task::states::{FoundInFollowing, FoundInPresent};
use chrono::{DateTime, Local};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use tokio::sync::watch;

pub(super) struct TsDuckInner {
    child: std::process::Child,
    pub(super) rx: watch::Receiver<EitDetected>,
    pub(super) stdin: std::process::ChildStdin,
}
impl Drop for TsDuckInner {
    fn drop(&mut self) {
        self.child.kill();
    }
}

#[derive(Debug)]
pub enum EitDetected {
    P(FoundInPresent),
    F(FoundInFollowing),
    NotFound { since: DateTime<Local> },
}

impl TsDuckInner {
    pub(crate) fn new() -> std::io::Result<Self> {
        let (tx, rx) = watch::channel(EitDetected::NotFound {
            since: Local::now(),
        });

        let mut child = {
            Command::new(format!("tstables"))
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .args([
                    "--japan",
                    "--log-json-line",
                    "--pid",
                    "0x12",
                    "--tid",
                    "0x4E",
                    "--section-number",
                    "0-1",
                ])
                .spawn()?
        };

        let stdin = child.stdin.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        std::thread::scope(|s| {
            s.spawn(|| {
                let mut reader = BufReader::new(stderr);

                let mut line: String = Default::default();
                while let Ok(n) = reader.read_line(&mut line) {
                    if n == 0 {
                        break;
                    }

                    match serde_json::from_str::<Value>(&line) {
                        Ok(data) => {
                            println!("Data: {:?}", data);
                            tx.send(EitDetected::F(FoundInFollowing {
                                start_at: Default::default(),
                                duration: None,
                            }))
                            .unwrap();
                        }
                        Err(e) => {
                            eprintln!("Error while parsing JSON: {:?}", e);
                        }
                    }
                }
            });
        });

        Ok(Self { stdin, child, rx })
    }
}
