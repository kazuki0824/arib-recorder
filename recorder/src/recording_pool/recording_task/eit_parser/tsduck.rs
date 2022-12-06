use std::io::{BufRead, BufReader, Error};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::RwLock;
use std::task::{Context, Poll};
use std::thread::ScopedJoinHandle;
use log::error;
use serde_json::Value;
use tokio::io::AsyncWrite;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::watch::Sender;
use tokio_process_stream::ProcessLineStream;
use crate::recording_pool::recording_task::eit_parser::{EitDetected, ParserInnerBase};
use crate::RecordingTaskDescription;

pub(super) struct TsDuckInner {
    child: ProcessLineStream,
    tx: Sender<EitDetected>,
}

impl ParserInnerBase for TsDuckInner {
    fn new(tx: Sender<EitDetected>) -> Result<Self, std::io::Error> {
        let child: ProcessLineStream = {
            Command::new(
                format!("tstables")
            )
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .args(["--flush", "--japan", "--log-json-line", "--pid", "0x12", "--tid", "0x4E"]) //, "--section-number", "0-1"])
                .kill_on_drop(true)
                .spawn()?
                .try_into()
        };

        Ok(Self {
            child,
            tx,
        })
    }
    // fn proc_output(&self, program: i64) -> Result<(), Error> {
    //     loop {
    //         let mut line;
    //         match self.reader.write().unwrap().read_line(&mut line) {
    //             Ok(0) => {
    //                 error!("Unexpected EOF on tstables");
    //                 return Ok(())
    //             }
    //             Ok(_) => {
    //                 let item: Value = serde_json::from_str(&line).unwrap();
    //                 panic!("{:#?}", item);
    //             }
    //             Err(e) => return Err(e)
    //         }
    //     }
    // }
}

impl AsyncWrite for TsDuckInner {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, Error>> {
        Pin::new(&mut self.get_mut().writer).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(&mut self.get_mut().writer).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(&mut self.get_mut().writer).poll_shutdown(cx)
    }
}
