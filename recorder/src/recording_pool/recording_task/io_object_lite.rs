use std::io::Write;
use std::mem::ManuallyDrop;
use std::path::Path;
use std::pin::Pin;
use std::process;
use std::process::Stdio;
use std::task::{Context, Poll};

use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone};
use futures_util::TryFutureExt;
use jsonpath_rust::JsonPathQuery;
use log::{error, info, warn};
use mirakurun_client::models::Program;
use serde_json::Value;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::io::BufWriter;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, LinesCodec, LinesCodecError};
use libc;

use crate::recording_pool::recording_task::states::{FoundInFollowing, FoundInPresent};
use crate::RecordingTaskDescription;

#[derive(Debug, Clone)]
pub enum EitDetected {
    P(FoundInPresent),
    F(FoundInFollowing),
    NotFound { since: DateTime<Local> },
}
pub(super) struct IoObjectLite {
    child: process::Child,
    pub(super) rx: mpsc::Receiver<EitDetected>,
    pub(super) stdin: tokio::process::ChildStdin
}

impl Drop for IoObjectLite {
    fn drop(&mut self) {
        info!("Killing {}...", self.child.id());

        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGINT);
        }
    }
}

impl AsyncWrite for IoObjectLite {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut self.get_mut().stdin).poll_write(cx, buf)
    }

    fn poll_flush(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.get_mut().stdin).poll_flush(cx)
    }

    fn poll_shutdown(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.get_mut().stdin).poll_shutdown(cx)
    }
}

impl IoObjectLite {
    pub fn poll_recv(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<EitDetected>> {
        self.get_mut().rx.poll_recv(cx)
    }

    pub fn new(output: Option<&Path>, info: Program) -> std::io::Result<Self>  {
        let (tx, rx) = mpsc::channel(100);

        info!("File open: {:?}", output);
        let output = output.unwrap_or(Path::new("/dev/null"));

        let mut child = process::Command::new("bash")
            .arg("-e")
            .arg("-o")
            .arg("pipefail")
            .arg("-c")
            .arg(format!(r"trap 'kill -TERM $!' SIGINT ; tee >(tstables --fill-eit --japan --log-json-line --pid 0x12 --tid 0x4E --section-number 0-1 --flush --no-pager) >(tsreadex -x 18/38/39 -n -1 -a 13 -b 5 -c 1 -u 1 -d 13 - >> {:?}) >(ffplay - &> /dev/null)", output))
            .stdin(Stdio::piped())
            .stdout(Stdio::null()) //TODO: Processed bytes
            .stderr(Stdio::piped())
            .spawn()?;

        info!("PID: {}", child.id());

        let stdin = tokio::process::ChildStdin::from_std(child.stdin.take().unwrap())?;
        let stderr = child.stderr.take().unwrap();

        // tstables reader
        tokio::spawn(async move {
            let stderr = tokio::process::ChildStderr::from_std(stderr).unwrap();
            let mut reader = FramedRead::new(stderr, LinesCodec::new());

            let info = info;

            let mut last = EitDetected::NotFound {
                since: Local::now(),
            };

            while let Some(line) = reader.next().await {
                let start = std::time::Instant::now();
                let line = line?;
                info!("{}", line);

                let info = info.clone();
                let cloned_tx = tx.clone();

                if let Some(send_value) = Self::derive_from_last_line(&last, &line, &info) {
                    info!("[id={}] Send: {:?}", info.id, send_value);
                    info!("[id={}] Sent! {} usecs elapsed.", info.id, start.elapsed().as_micros());
                    last = send_value.clone();

                    tokio::spawn(async move {
                        /* Send result across thread */
                        cloned_tx.send(send_value).await.map_err(|e| error!("{:?}", e))
                    });
                }
            }

            warn!("tstables' stderr reached EOF.");
            Ok::<(), LinesCodecError>(())
        });

        Ok(Self { stdin, child, rx })
    }

    fn derive_from_last_line(last: &EitDetected, line: &str, p: &Program) -> Option<EitDetected> {
        // scan each lines until found.
        let mut result = None;
        for line in line.trim_end().split('\n') {
            match serde_json::from_str::<Value>(line) {
                Ok(body) => {
                    if let Some(eit) = Self::extract_eit2(&body, p).unwrap() {
                        result = Some(eit);
                        break;
                    }
                }
                Err(e) => {
                    warn!("Error while parsing tstables' output: {:?}", e);
                    // log
                    let mut w = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .write(true)
                        .open(format!("./logs/{}.log", p.name.as_ref().unwrap()))
                        .unwrap();
                    let _ = writeln!(w, "{:?}", e);
                    let _ = writeln!(w, "{}", line);
                    return None;
                }
            };
        }
        info!("{:?}", result);
        // Assume result
        let result_merged = match (&last, &result) {
            (
                EitDetected::NotFound { since },
                Some(EitDetected::NotFound { .. }),
            ) => EitDetected::NotFound {
                since: *since,
            },
            (_, None) => return None,
            _ => result.unwrap(),
        };

        Some(result_merged)
    }

    fn extract_eit2(body: &Value, info: &Program) -> Result<Option<EitDetected>, String> {
        // 抽出
        // None: This line has different nid or sid so ignored.
        // Some(EitDetected::NotFound): Nid and sid matched but the event is not found.
        if let (
            Value::Array(nid),
            Value::Array(sid),
            Value::Array(ty),
            Value::Array(eids),
            Value::Array(starts),
            Value::Array(durations),
        ) = (
            body.clone().path("$.original_network_id")?,
            body.clone().path("$.service_id")?,
            body.clone().path("$.type")?,
            body.clone().path("$.#nodes.*.event_id")?,
            body.clone().path("$.#nodes.*.start_time")?,
            body.clone().path("$.#nodes.*.duration")?,
        ) {
            // 照合
            match (nid.first(), sid.first(), ty.first()) {
                (Some(nid), Some(sid), Some(ty))
                    if nid == info.network_id && sid == info.service_id && ty == "pf" =>
                {
                    let mut i = 0;

                    let mut iterator = eids.iter().zip(starts.iter());
                    while let Some((Value::Number(eid), Value::String(start))) = iterator.next() {
                        if info.event_id as i64 == eid.as_i64().unwrap() {
                            info!("hit");
                            let parsed_start =
                                NaiveDateTime::parse_from_str(start, "%Y-%m-%d %H:%M:%S")
                                    .ok()
                                    .and_then(|t| Local.from_local_datetime(&t).single());
                            let duration = durations.get(i).map(|s| {
                                let mut reverse_split = s.as_str().unwrap().rsplit(':');
                                let sec = reverse_split
                                    .next()
                                    .and_then(|s| s.parse::<i64>().ok())
                                    .unwrap_or(0);
                                let min = reverse_split
                                    .next()
                                    .and_then(|s| s.parse::<i64>().ok())
                                    .unwrap_or(0);
                                let hour = reverse_split
                                    .next()
                                    .and_then(|s| s.parse::<i64>().ok())
                                    .unwrap_or(0);

                                let sec = Duration::seconds(sec);
                                let min = Duration::minutes(min);
                                let hour = Duration::hours(hour);

                                sec + min + hour
                            });

                            let start_at = parsed_start.unwrap();
                            if Local::now() < start_at {
                                // EIT[following]
                                return Ok(Some(EitDetected::F(FoundInFollowing {
                                    start_at,
                                    duration,
                                })));
                            } else if duration.is_some()
                                && Local::now() < start_at + duration.unwrap() + Duration::seconds(30)
                            {
                                // EIT[present]
                                return Ok(Some(EitDetected::P(FoundInPresent {
                                    start_at,
                                    duration,
                                })));
                            } else {
                                error!("[BUG] hit but ended. TOT calibration needed.");
                                continue;
                            }
                        }

                        i += 1;
                    }
                    Ok(Some(EitDetected::NotFound {
                        since: Local::now(),
                    }))
                }
                _ => Ok(None),
            }
        } else {
            unreachable!("")
        }
    }
}
