use std::io::{BufWriter, Write};
use std::process::{Command, Stdio};

use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone};
use jsonpath_rust::JsonPathQuery;
use log::{error, info, warn};
use mirakurun_client::models::Program;
use serde_json::Value;
use tokio::sync::watch;
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, LinesCodec, LinesCodecError};

use crate::recording_pool::recording_task::states::{FoundInFollowing, FoundInPresent};

pub(super) struct TsDuckInner {
    child: std::process::Child,
    pub(super) rx: watch::Receiver<EitDetected>,
    pub(super) stdin: BufWriter<std::process::ChildStdin>,
}

impl Drop for TsDuckInner {
    fn drop(&mut self) {
        self.child.kill();
    }
}

#[derive(Debug, Clone)]
pub enum EitDetected {
    P(FoundInPresent),
    F(FoundInFollowing),
    NotFound { since: DateTime<Local> },
}

impl TsDuckInner {
    pub(crate) fn new(info: Program) -> std::io::Result<Self> {
        let (tx, rx) = watch::channel(EitDetected::NotFound {
            since: Local::now(),
        });

        let mut child = {
            Command::new("tstables")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .args([
                    "--flush",
                    "--japan",
                    "--log-json-line",
                    "--pid",
                    "0x12",
                    "--tid",
                    "0x4E",
                    "--section-number",
                    "0-1",
                    "--no-pager",
                ])
                .spawn()?
        };

        let stdin = BufWriter::new(child.stdin.take().unwrap());
        let stderr = child.stderr.take().unwrap();

        // TODO: 5sec trigger for A->B1

        // tstables reader
        tokio::spawn(async move {
            let stderr = tokio::process::ChildStderr::from_std(stderr).unwrap();
            let mut reader = FramedRead::new(stderr, LinesCodec::new());

            let info = info;

            while let Some(line) = reader.next().await {
                let line = line?;
                info!("{}", line);

                let to_be_written = {
                    // scan each lines until found.
                    let mut result = None;
                    for line in line.trim_end().split('\n') {
                        match serde_json::from_str::<Value>(line) {
                            Ok(body) => {
                                if let Some(eit) = Self::extract_eit2(&body, &info).unwrap() {
                                    result = Some(eit);
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("Error while parsing tstables' output: {:?}", e);
                                let mut w = std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .write(true)
                                    .open(format!("./logs/{}.log", info.name.as_ref().unwrap()))
                                    .unwrap();
                                let _ = writeln!(w, "{:?}", e);
                                let _ = writeln!(w, "{}", line);
                                continue;
                            }
                        };
                    }

                    // Assume result
                    match (&*tx.borrow(), &result) {
                        (
                            EitDetected::NotFound { since },
                            Some(EitDetected::NotFound { .. }) | None,
                        ) => EitDetected::NotFound {
                            since: *since,
                        },
                        (_, None) => continue,
                        _ => result.unwrap(),
                    }
                };

                info!("[id={}] Send: {:?}", info.id, to_be_written);
                tx.send(to_be_written).map_err(|e| error!("{:?}", e));
            }

            warn!("tstables' stderr reached EOF.");
            Ok::<(), LinesCodecError>(())
        });

        Ok(Self { stdin, child, rx })
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
                                && Local::now() < start_at + duration.unwrap()
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
