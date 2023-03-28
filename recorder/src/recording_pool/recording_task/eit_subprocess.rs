use std::io::{BufWriter, Write};
use std::process::{Command, Stdio};

use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone};
use log::{error, info, warn};
use mirakurun_client::models::Program;
use serde_json::{Map, Value};
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

#[derive(Debug)]
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
            Command::new(format!("stdbuf"))
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .args([
                    "-eL",
                    "tstables",
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

        tokio::spawn(async move {
            let stderr = tokio::process::ChildStderr::from_std(stderr).unwrap();
            let mut reader = FramedRead::new(stderr, LinesCodec::new());

            let info = info;

            while let Some(line) = reader.next().await {
                let line = line?;
                info!("{}", line);

                let to_be_written = {
                    // scan each lines until found
                    let mut result = None;
                    for line in line.trim_end().split('\n') {
                        match serde_json::from_str::<Value>(&line) {
                            Ok(Value::Object(ref body)) => {
                                let (nid, sid) = (
                                    body["original_network_id"].as_i64().unwrap(),
                                    body["service_id"].as_i64().unwrap(),
                                );
                                if let Some(eit) = Self::extract_eit(body, &info) {
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
                                writeln!(w, "{:?}", e);
                                writeln!(w, "{}", line);
                                continue;
                            }
                            _ => todo!(),
                        };
                    }

                    // Assume result
                    match (&*tx.borrow(), &result) {
                        (EitDetected::NotFound { since }, None) => EitDetected::NotFound {
                            since: since.clone(),
                        },
                        (_, None) => EitDetected::NotFound {
                            since: Local::now(),
                        },
                        _ => result.unwrap(),
                    }
                };

                info!("[id={}] Send: {:?}", info.id, to_be_written);
                tx.send_replace(to_be_written);
            }

            warn!("tstables' stderr reached EOF.");
            Ok::<(), LinesCodecError>(())
        });

        Ok(Self { stdin, child, rx })
    }

    fn extract_eit(body: &Map<String, Value>, info: &Program) -> Option<EitDetected> {
        if let Some(eits) = body["#nodes"].as_array() {
            let filtered = eits.iter().filter_map(|eit| eit.as_object()).filter(|eit| {
                eit.contains_key("event_id")
                    && eit.contains_key("start_time")
                    && eit.contains_key("duration")
            });

            let mut result: Option<EitDetected> = None;

            for item in filtered {
                info!("[Id={}] Received EIT{:?}", info.id, item);

                let eid = item.get("event_id").unwrap().as_i64().unwrap();
                // let id = 10000000000 * nid + 100000 * sid + eid;

                if eid == info.id % 100000 {
                    info!("hit");
                    let start_at = item
                        .get("start_time")
                        .unwrap()
                        .as_str()
                        .unwrap_or("not found");
                    let duration = item.get("duration").and_then(|f| f.as_str());

                    let parsed_start = NaiveDateTime::parse_from_str(start_at, "%Y-%m-%d %H:%M:%S")
                        .ok()
                        .and_then(|t| Local.from_local_datetime(&t).single());
                    if let Some(start_at) = parsed_start {
                        // send result
                        let duration = duration.map(|s| {
                            let mut reverse_split = s.rsplit(':');
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
                        if Local::now() < start_at {
                            // EIT[following]
                            result = Some(EitDetected::F(FoundInFollowing { start_at, duration }));
                        } else if duration.is_some() && Local::now() < start_at + duration.unwrap()
                        {
                            // EIT[present]
                            result = Some(EitDetected::P(FoundInPresent { start_at, duration }));
                        } else {
                            error!("[BUG] hit but ended. TOT calibration needed.")
                        }
                        break;
                    } else {
                        continue;
                    }
                }
            }
            result
        } else {
            None
        }
    }
}
