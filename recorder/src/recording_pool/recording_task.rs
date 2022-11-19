use std::io::Error;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};

use chrono::{DateTime, Duration, Local};
use log::info;
use machine::{machine, transitions};
use pin_project_lite::pin_project;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::recording_pool::recording_task::eit_parser::*;
use crate::recording_pool::recording_task::io_object::IoObject;
use crate::recording_pool::REC_POOL;
use crate::RecordingTaskDescription;

mod eit_parser;
mod io_object;

machine!(
    #[derive(Clone, Copy, PartialEq, Debug)]
    pub(crate) enum RecordingState {
        A {
            since: DateTime<Local>,
        },
        B1 {
            since: DateTime<Local>,
        },
        B2 {
            since: DateTime<Local>,
        },
        Rec {
            since: DateTime<Local>,
            end: DateTime<Local>,
        },
        Timeout {},
        Success {},
    }
);
impl IntoB2 for A {}
impl IntoB2 for B1 {}

impl IntoRec for A {}
impl IntoRec for B1 {}
impl IntoRec for B2 {}

trait IntoB2 {
    fn on_found_in_following(self, _: FoundInFollowing) -> B2
    where
        Self: Sized,
    {
        B2 {
            since: Local::now(),
        }
    }
}

trait IntoRec {
    fn on_found_in_present(self, present: FoundInPresent) -> Rec
    where
        Self: Sized,
    {
        Rec {
            since: Local::now(),
            end: present.duration.map_or(Local::now() + Duration::minutes(5), |d| present.start_at + d),
        }
    }
}

impl A {
    fn on_wait_for_premiere(self, WaitForPremiere { start_at }: WaitForPremiere) -> RecordingState {
        if start_at < Local::now() {
            RecordingState::B1(B1 {
                since: Local::now(),
            })
        } else if self.since + Duration::hours(1) < Local::now() {
            RecordingState::Timeout(self::Timeout {})
        } else {
            RecordingState::A(A { since: self.since })
        }
    }
}
impl B1 {
    fn on_wait_for_premiere(self, WaitForPremiere { start_at }: WaitForPremiere) -> RecordingState {
        if self.since + Duration::hours(3) < Local::now() {
            RecordingState::Timeout(self::Timeout {})
        } else {
            RecordingState::B1(B1 { since: self.since })
        }
    }
}
impl B2 {
    fn on_found_in_following(self, _: FoundInFollowing) -> B2 {
        self
    }
}
impl Rec {
    fn on_found_in_present(self, _: FoundInPresent) -> Rec {
        self
    }
    fn on_wait_for_premiere(self, WaitForPremiere { start_at }: WaitForPremiere) -> Success {
        Success {}
    }
}

transitions!(RecordingState,
    [
        (A, FoundInPresent) => Rec,
        (B1, FoundInPresent) => Rec,
        (B2, FoundInPresent) => Rec,
        (A, WaitForPremiere) => [A, B1],
        (B1, WaitForPremiere) => [B1, Timeout],
        (Rec, WaitForPremiere) => Success,
        (A, FoundInFollowing) => B2,
        (B1, FoundInFollowing) => B2,
        (B2, FoundInFollowing) => B2,
        (A, FoundInPresent) => Rec,
        (B1, FoundInPresent) => Rec,
        (B2, FoundInPresent) => Rec,
        (Rec, FoundInPresent) => Rec
    ]
);

#[derive(Clone, Debug, PartialEq)]
pub struct FoundInFollowing {
    start_at: DateTime<Local>,
    duration: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FoundInPresent {
    start_at: DateTime<Local>,
    duration: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WaitForPremiere {
    start_at: DateTime<Local>,
}

pin_project! {
    pub(crate) struct RecordingTask {
        #[pin]
        target: Option<IoObject>,
        eit: EitParser,
        next_state: RecordingState,
        pub(crate) state: RecordingState,
        pub(crate) id: i64,
        pub(crate) file_location: PathBuf
    }
}

impl RecordingTask {
    pub(crate) async fn new(info: &RecordingTaskDescription) -> Result<Self, Error> {
        let info = info.clone();
        info!("Create a new recording task: {:?}", info);
        let mut file_location = info.save_dir_location;
        // Specify file name here
        file_location.push(format!(
            "{}_{}.m2ts-tmp",
            info.program.event_id,
            info.program
                .name
                .as_ref()
                .unwrap_or(&"untitled".to_string())
        ));
        let target = Some(IoObject::new(None).await?);
        Ok(Self {
            target,
            eit: EitParser::new(),
            next_state: RecordingState::A(A {
                since: Local::now(),
            }),
            state: RecordingState::A(A {
                since: Local::now(),
            }),
            id: info.program.id,
            file_location,
        })
    }
}

impl AsyncWrite for RecordingTask {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut me = self.project();

        // Get RecordingDescription. If not exist, return error.
        if let Some(item) = REC_POOL.read().unwrap().at(me.id) {
            // Evaluate states and control IoObject
            let after = match me.eit.push(buf, item) {
                EitDetected::P(inner) => me.state.on_found_in_present(inner),
                EitDetected::F(inner) => me.state.on_found_in_following(inner),
                EitDetected::NotFound { since } => {
                    if Local::now() - since > Duration::seconds(30) {
                        //停波中
                        todo!()
                    } else {
                        me.state.on_wait_for_premiere(WaitForPremiere {
                            start_at: REC_POOL
                                .read()
                                .unwrap()
                                .at(me.id)
                                .unwrap()
                                .program
                                .start_at
                                .into(),
                        })
                    }
                }
            };
            *me.next_state = after;

            if me.state != me.next_state {
                info!(
                    "[id={}] Transition from {:?} to {:?}",
                    item.program.id, me.state, me.next_state
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
                            let new_writer = IoObject::new(new_path).await.unwrap();
                            if let Some(mut old_writer) = me.target.replace(new_writer) {
                                old_writer.shutdown().await.unwrap()
                            }
                            info!(
                                "[id={}] Save location has been changed to {:?}.",
                                item.program.id, new_path
                            );
                            w.wake()
                        };
                        rt.block_on(future)
                    });
                });

                Poll::Pending
            } else {
                me.target.as_pin_mut().unwrap().poll_write(cx, buf)
            }
        } else {
            me.target
                .as_pin_mut()
                .unwrap()
                .poll_shutdown(cx)
                .map_ok(|_| 0)
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let me = self.project();
        me.target.as_pin_mut().unwrap().poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let me = self.project();
        REC_POOL.write().unwrap().try_remove(me.id);
        info!("id: {} is shutting down...", me.id);
        me.target.as_pin_mut().unwrap().poll_shutdown(cx)
    }
}
