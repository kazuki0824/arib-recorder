use crate::recording_pool::recording_task::eit_parser::EitDetected;
use crate::RecordingTaskDescription;
use serde_json::Value;
use std::io;
use std::pin::Pin;
use std::process::{Command, Stdio};
use tokio::io::{AsyncBufReadExt, AsyncWrite};
use tokio::sync::watch::Sender;

pub(super) struct TsDuckInner {
    stdin: tokio::process::ChildStdin,
    child: std::process::Child,

    tx: Sender<EitDetected>,
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
                .stdout(Stdio::piped())
                .args([
                    "--flush",
                    "--japan",
                    "--log-json-line",
                    "--pid",
                    "0x12",
                    "--tid",
                    "0x4E",
                ]) //, "--section-number", "0-1"])
                .spawn()?
        };

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stdin = tokio::process::ChildStdin::from_std(stdin)?;
        let stdout = tokio::process::ChildStdout::from_std(stdout)?;
        let mut reader = tokio::io::BufReader::new(stdout).lines();

        tokio::spawn(async move {
            while let Some(line) = reader.next_line().await? {
                match serde_json::from_str::<Value>(&line) {
                    Ok(data) => {
                        println!("Data: {:?}", data);
                    }
                    Err(e) => {
                        eprintln!("Error while parsing JSON: {:?}", e);
                    }
                }
            }
            Ok::<_, io::Error>(())
        });

        Ok(Self { stdin, child, tx })
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
