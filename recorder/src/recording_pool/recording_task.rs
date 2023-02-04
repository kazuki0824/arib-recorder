mod eit_subprocess;
mod io_object;
mod states;

use crate::recording_pool::recording_task::eit_subprocess::{EitDetected, TsDuckInner};
use crate::recording_pool::recording_task::io_object::IoObject;
use crate::recording_pool::recording_task::states::{RecordingState, WaitForPremiere, A};
use crate::recording_pool::REC_POOL;
use crate::RecordingTaskDescription;
use chrono::{Duration, Local};
use futures_util::TryStreamExt;
use log::{debug, error, info};
use mirakurun_client::apis::configuration::Configuration;
use mirakurun_client::apis::programs_api::get_program_stream;
use pin_project_lite::pin_project;
use std::future::Future;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use tokio::io::{AsyncBufRead, AsyncWrite, AsyncWriteExt};
use tokio_util::io::StreamReader;

pub enum RecExitType {
    Aborted(u64),
    Success(u64),
}

pin_project! {
    pub(crate) struct RecTask {
        #[pin]
        rec: Option<IoObject>,
        src: Box<dyn AsyncBufRead + Unpin + Send>,
        amt: u64,
        poll_count: u64,
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

            let mut c = Configuration::new();
            c.base_path = m_url.to_string();
            // Get Ts Stream
            let src =
                match get_program_stream(&c, info.program.id, None, None).await {
                    Ok(value) => StreamReader::new(value.bytes_stream().map_err(
                        |e: mirakurun_client::Error| io::Error::new(std::io::ErrorKind::Other, e),
                    )),
                    Err(e) => return Err(io::Error::new(std::io::ErrorKind::Other, e)),
                };

            //Eit
            let eit = TsDuckInner::new(info.program)?;

            (src, rec, file_location, eit)
        };

        info!("[Id={}]Connection successful.", id);

        Ok(Self {
            rec,
            src: Box::new(src),
            amt: 0,
            poll_count: 0,
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
        *me.poll_count += 1;

        while let Some(item) = REC_POOL.get(me.id).map(|c| c.val().clone()) {
            //TODO: If A, read Mirakurun

            // State transition
            if me.state != me.next_state {
                info!(
                    "[id={}] Transition from {:?} to {:?}",
                    me.id, me.state, me.next_state
                );
                // Determine file name
                let new_path = match me.next_state {
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

                info!(
                    "[id={}] A new writer will be used from next polling...",
                    me.id
                );
                return Poll::Pending;
            }

            // Evaluate states and control IoObject
            if me.eit.rx.has_changed().unwrap() {
                let after = match *me.eit.rx.borrow_and_update() {
                    EitDetected::P(ref inner) => me.state.on_found_in_present(inner.clone()),
                    EitDetected::F(ref inner) => me.state.on_found_in_following(inner.clone()),
                    EitDetected::NotFound { since } => {
                        if Local::now() - since > Duration::seconds(30) {
                            //停波中
                            error!(
                                "No EIT received for 30 secs. Check the child process and signal"
                            );
                            break;
                        } else {
                            me.state.on_wait_for_premiere(WaitForPremiere {
                                start_at: item.program.start_at.into(),
                            })
                        }
                    }
                };
                *me.next_state = after;

                info!("[id={}] State will be updated in the next loop", me.id);
            }

            // 読み取りの試行
            let buffer = ready!(Pin::new(&mut me.src).poll_fill_buf(cx))?;
            if buffer.is_empty() {
                ready!(me.rec.as_pin_mut().unwrap().poll_flush(cx))?;

                info!(
                    "[id={}] Recording has finished, and the buffer is successfully flushed.",
                    me.id
                );
                REC_POOL.remove(me.id);

                return Poll::Ready(Ok(RecExitType::Success(*me.amt)));
            }

            // tstablesへ書き込み
            me.eit
                .stdin
                .write_all(buffer)
                .expect("Writing to subprocess failed.");
            // me.eit.stdin.flush().expect("Writing to subprocess failed.");

            // 書き込みの試行
            let i = ready!(me.rec.as_mut().as_pin_mut().unwrap().poll_write(cx, buffer))?;
            debug!(
                "[id={}] {} bytes has been writen to {}",
                me.id,
                i,
                me.file_location.display()
            );
            if i == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            *me.amt += i as u64;
            Pin::new(&mut *me.src).consume(i);
        }

        info!("[id={}] Aborted.", me.id);
        REC_POOL.remove(me.id);
        Poll::Ready(Ok(RecExitType::Aborted(*me.amt)))
    }
}
