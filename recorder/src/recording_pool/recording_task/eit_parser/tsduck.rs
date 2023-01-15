use crate::recording_pool::recording_task::eit_parser::EitDetected;
use crate::RecordingTaskDescription;
use serde_json::Value;
use std::io;
use std::pin::Pin;
use std::process::{Command, Stdio};
use tokio::io::{AsyncBufReadExt, AsyncWrite};
use tokio::sync::watch::Sender;
use crate::recording_pool::recording_task::FoundInFollowing;

pub(super) struct TsDuckInner {
    stdin: tokio::process::ChildStdin,
    child: std::process::Child,
}
impl Drop for TsDuckInner {
    fn drop(&mut self) {
        self.child.kill();
    }
}

impl TsDuckInner {
    pub(crate) fn new(tx: Sender<EitDetected>) -> io::Result<Self> {
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
                    "0-1"
                ])
                .spawn()?
        };

        let stdin = child.stdin.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let stdin = tokio::process::ChildStdin::from_std(stdin)?;
        let stderr = tokio::process::ChildStderr::from_std(stderr)?;
        let mut reader = tokio::io::BufReader::new(stderr).lines();

        tokio::spawn(async move {
            while let Some(line) = reader.next_line().await? {
                match serde_json::from_str::<Value>(&line) {
                    Ok(data) => {
                        println!("Data: {:?}", data);
                        tx.send(EitDetected::F(FoundInFollowing { start_at: Default::default(), duration: None })).unwrap();
                    }
                    Err(e) => {
                        eprintln!("Error while parsing JSON: {:?}", e);
                    }
                }
            }
            Ok::<_, io::Error>(())
        });

        Ok(Self { stdin, child })
    }
}

impl AsyncWrite for TsDuckInner {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().stdin).poll_write(cx, buf)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stdin).poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stdin).poll_shutdown(cx)
    }
}
