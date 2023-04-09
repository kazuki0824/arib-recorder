use std::future::Future;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::pin::{pin, Pin};
use std::task::{ready, Context, Poll};

use chrono::{Duration, Local};
use futures_util::{FutureExt, TryStreamExt};
use log::{error, info, warn};
use mirakurun_client::apis::configuration::Configuration;
use mirakurun_client::apis::programs_api::get_program_stream;
use mirakurun_client::apis::services_api::get_service_stream;
use mirakurun_client::models::related_item::Type;
use pin_project_lite::pin_project;
use tokio::io::{AsyncBufRead, AsyncWrite, AsyncWriteExt};
use tokio_util::io::StreamReader;

use crate::recording_pool::recording_task::eit_subprocess::{EitDetected, TsDuckInner};
use crate::recording_pool::recording_task::io_object::IoObject;
use crate::recording_pool::recording_task::states::{RecordingState, WaitForPremiere, A};
use crate::recording_pool::REC_POOL;
use crate::RecordingTaskDescription;

mod eit_subprocess;
mod io_object;
mod states;

pub enum RecExitType {
    Aborted(u64),
    Continue(i64, i64, i64, u64),
    Success(u64),
    EndOfStream(u64),
}

pin_project! {
    pub(crate) struct RecTask {
        #[pin]
        rec: Option<IoObject>,
        src: Box<dyn AsyncBufRead + Unpin + Send>,
        amt: u64,
        start: std::time::Instant,
        eit: TsDuckInner,
        next_state: RecordingState,
        pub(crate) state: RecordingState,
        pub(crate) id: i64,
        pub(crate) file_location: PathBuf
    }
}

impl RecTask {
    pub(crate) async fn new(m_url: String, info: RecordingTaskDescription) -> io::Result<Self> {
        let id = {
            if REC_POOL.iter().any(|v| *v.key() == info.program.id) {
                panic!("Already found")
            } else {
                REC_POOL.insert(info.program.id, info.clone());
                info.program.id
            }
        };
        info!("Create a new recording task: {:?}", info);

        let (src, rec, file_location, eit) = {
            // Output location
            let mut file_location = info.save_dir_location;
            let filename = format!(
                "{}_{}.m2ts-tmp",
                info.program.event_id,
                info.program
                    .name
                    .as_ref()
                    .unwrap_or(&"untitled".to_string())
                    .chars()
                    .map(|c| if "[\\\\/:*?\"<>|]".contains(c) {
                        ' '
                    } else {
                        c
                    })
                    .collect::<String>()
            );
            // Specify file name here
            file_location.push(filename);

            // Create a new task
            let rec = Some(IoObject::new(None).await?);

            // Get Ts Stream
            let mut c = Configuration::new();
            c.base_path = m_url.to_string();
            let src = if let Some((nid, sid, eid)) = info.id_override {
                // Relayed
                let id = nid * 10000000000 + sid * 100000 + eid;
                match get_program_stream(&c, id, None, None).await {
                    Ok(value) => Box::new(StreamReader::new(value.bytes_stream().map_err(
                        |e: mirakurun_client::Error| io::Error::new(std::io::ErrorKind::Other, e),
                    ))) as Box<dyn AsyncBufRead + Unpin + Send>,
                    Err(e) => {
                        return Err(io::Error::new(std::io::ErrorKind::Other, e));
                    }
                }
            } else {
                // Direct
                let id = info.program.id / 100000;
                match get_service_stream(&c, id, None, None).await {
                    Ok(value) => Box::new(StreamReader::new(value.bytes_stream().map_err(
                        |e: mirakurun_client::Error| io::Error::new(std::io::ErrorKind::Other, e),
                    ))) as Box<dyn AsyncBufRead + Unpin + Send>,
                    Err(e) => {
                        return Err(io::Error::new(std::io::ErrorKind::Other, e));
                    }
                }
            };

            //Eit
            let eit = TsDuckInner::new(info.program)?;

            (src, rec, file_location, eit)
        };

        info!("[Id={}]Connection successful.", id);

        Ok(Self {
            rec,
            src,
            amt: 0,
            start: std::time::Instant::now(),
            eit,
            next_state: RecordingState::A(A {
                since: Local::now(),
            }),
            state: RecordingState::A(A {
                since: Local::now(),
            }),
            id,
            file_location,
        })
    }
}

impl Future for RecTask {
    type Output = io::Result<RecExitType>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut me = self.project();

        if let Some(item) = REC_POOL.get(me.id).map(|c| c.val().clone()) {
            // State transition
            if me.state != me.next_state {
                info!(
                    "[id={}] Transition from {:?} to {:?}",
                    me.id, me.state, me.next_state
                );

                // Determine file name
                let new_path = match me.next_state {
                    RecordingState::Error => {
                        todo!()
                    }
                    RecordingState::Success(_) => {
                        // If next id is found, continue.
                        info!("[id={}] Reached Success.", me.id);
                        let relay = REC_POOL
                            .get(me.id)
                            .and_then(|v| v.val().program.related_items.clone())
                            .and_then(|v| {
                                v.into_iter().find_map(|item| {
                                    if matches!(item.r#type, Some(Type::Relay)) {
                                        Some(item)
                                    } else {
                                        None
                                    }
                                })
                            });
                        // 正常離脱（リレー/終了）
                        return if let Some(next) = relay {
                            Poll::Ready(Ok(RecExitType::Continue(
                                next.network_id.unwrap(),
                                next.service_id.unwrap(),
                                next.event_id.unwrap(),
                                *me.amt,
                            )))
                        } else {
                            Poll::Ready(Ok(RecExitType::Success(*me.amt)))
                        };
                    }
                    RecordingState::Rec(_) if me.file_location.set_extension("m2ts") => {
                        Some(me.file_location.as_path())
                    }
                    RecordingState::B1(_) | RecordingState::B2(_)
                        if me.file_location.set_extension("m2ts-tmp") =>
                    {
                        Some(me.file_location.as_path())
                    }
                    _ => None,
                };
                info!(
                    "[id={}] A new writer will be used from next polling...",
                    me.id
                );
                *me.state = *me.next_state;

                // Kill the current IoObject and create a new one
                std::thread::scope(|s| {
                    let w = cx.waker().clone();

                    s.spawn(|| {
                        // Create the runtime
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        // Execute the future, blocking the current thread until completion
                        let future = async {
                            // Create new. Only when error occurred, it can be `None`.
                            let new_writer = match IoObject::new(new_path).await {
                                Ok(w) => Some(w),
                                Err(e) => {
                                    error!("{:#?}", e);
                                    None
                                }
                            };

                            // Replacement
                            let old_writer = match new_writer {
                                None => me.rec.take(),
                                Some(new_writer) => me.rec.replace(new_writer),
                            };

                            // Shutdown
                            if let Some(mut old_writer) = old_writer {
                                old_writer.shutdown().await.unwrap()
                            }
                            info!(
                                "[id={}] Save location has been changed to {:?}.",
                                me.id, new_path
                            );
                            w.wake()
                        };
                        rt.block_on(future)
                    });
                });

                return Poll::Pending;
            }

            //ピン留め
            let rec = me.rec.as_pin_mut();

            // 読み取りの試行
            let buffer = ready!(Pin::new(&mut me.src).poll_fill_buf(cx))?;
            if buffer.is_empty() {
                // 読み取り結果がEOFの場合、ライターがあればフラッシュして終了
                if let Some(rec) = rec {
                    ready!(rec.poll_flush(cx))?;
                }
                info!(
                    "[id={}] Recording has finished, and the buffer is successfully flushed.",
                    me.id
                );
                return Poll::Ready(Ok(RecExitType::EndOfStream(*me.amt)));
            }

            // tstablesへ書き込み
            me.eit
                .stdin
                .write_all(buffer)
                .expect("Writing to subprocess failed.");
            // me.eit
            //     .stdin
            //     .flush()
            //     .expect("Writing to subprocess failed.");

            // 書き込みの試行
            if let Some(rec) = rec {
                let i = ready!(rec.poll_write(cx, buffer))?;
                // log::debug!(
                //     "[id={}] {} bytes has been written to {}",
                //     me.id,
                //     i,
                //     me.file_location.display()
                // );
                if i == 0 {
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                *me.amt += i as u64;
                pin!(&mut me.src).consume(i);
            } else {
                let i = buffer.len();
                pin!(&mut me.src).consume(i);
            }

            // Evaluate states and control IoObject
            let operator = |recv: EitDetected, state: RecordingState| {
                match recv {
                    EitDetected::P(ref inner) => {
                        state.on_found_in_present(inner.clone())
                    }
                    EitDetected::F(ref inner) => {
                        state.on_found_in_following(inner.clone())
                    }
                    EitDetected::NotFound { since } => {
                        if Local::now() - since > Duration::seconds(30) {
                            //停波中
                            warn!(
                                "No EIT received for 30 secs. Check the child process and signal"
                            );
                        }
                        state.on_wait_for_premiere(WaitForPremiere {
                            start_at: item.program.start_at.into(),
                        })
                    }
                }
            };

            cx.waker().wake_by_ref();

            // Check channel
            // First, exit immediately if pending
            let t = std::time::Instant::now();
            if let Some(recv) = ready!(me.eit.rx.poll_recv(cx)) {
                let mut after = operator(recv, *me.state);
                while let Poll::Ready(recv) = me.eit.rx.poll_recv(cx) {
                    after = operator(recv.unwrap(), after);
                }
                *me.next_state = after;

                info!("[id={}] State will be updated in the next loop", me.id);
                info!("[id={}] Next is {:?}", me.id, after);
            }
            let end = t.elapsed();
            // println!("Poll time: {} usec", end.as_micros());
            Poll::Pending
        } else {
            info!("[id={}] Aborted.", me.id);
            Poll::Ready(Ok(RecExitType::Aborted(*me.amt)))
        }
    }
}
