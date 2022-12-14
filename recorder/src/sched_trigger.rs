use std::io::Error;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Duration, Local};
use log::{debug, error, info, warn};
use mirakurun_client::models::Program;
use serde_derive::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;
use ulid::Ulid;

use crate::context::Context;
use crate::recording_planner::PlanUnit;
use crate::{RecordControlMessage, RecordingTaskDescription};

pub(crate) struct SchedQueue {
    pub(crate) items: Vec<Schedule>,
    save_file_location: PathBuf,
}

impl SchedQueue {
    pub fn new(location: Option<PathBuf>) -> Result<Self, Error> {
        let mut path = location.unwrap_or(PathBuf::from("./q_schedules.json"));

        //Import all the previously stored schedules
        let schedules = if path.exists() {
            path = path.canonicalize()?;
            let str = std::fs::read(&path)?;
            match serde_json::from_slice::<Vec<Schedule>>(&str) {
                Ok(items) => Some(items),
                Err(e) => {
                    warn!("q_schedules parse error.");
                    warn!("{}", e);
                    None
                }
            }
        } else {
            None
        };
        let schedules = schedules.unwrap_or_else(|| {
            info!("No valid q_schedules.json is found. It'll be created or overwritten just before exiting.");
            Vec::new()
        });
        Ok(Self {
            items: schedules,
            save_file_location: path,
        })
    }
    fn save(&mut self) {
        //Export remaining tasks
        let path = self
            .save_file_location
            .canonicalize()
            .unwrap_or(PathBuf::from("./q_schedules.json"));
        let result = match serde_json::to_string(&self.items) {
            Ok(str) => std::fs::write(&path, str),
            Err(e) => panic!("Serialization failed. {}", e),
        };
        if result.is_ok() {
            println!("q_schedules is saved in {}.", path.display())
        }
    }
}

impl Drop for SchedQueue {
    fn drop(&mut self) {
        self.save()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Schedule {
    pub(crate) program: Program,
    // If it is added through a plan (e.g. Record all of the items in the series), its uuid is stored here.
    pub(crate) plan_id: Option<(Ulid, PlanUnit)>,
    pub(crate) is_active: bool,
}

pub(crate) async fn sched_trigger_startup(
    cx: Arc<Context>,
    tx: Sender<RecordControlMessage>,
) -> Result<(), Error> {
    loop {
        debug!("Now locking q_schedules.");
        {
            let mut q_schedules = cx.q_schedules.write().unwrap();

            let (found, remainder) = {
                let found = q_schedules.items.len();

                // Drop expired item
                q_schedules.items.retain(|item| {
                    let start_at = item.program.start_at;

                    match item {
                        Schedule {
                            program:
                                Program {
                                    duration: Some(length_msec),
                                    ..
                                },
                            ..
                        } => {
                            //????????????????????????????????????????????????????????????
                            Local::now() < start_at + Duration::milliseconds(*length_msec as i64)
                        }
                        Schedule {
                            program: Program { duration: None, .. },
                            ..
                        } => {
                            //?????????????????????????????????????????????????????????????????????????????????
                            Local::now() < start_at + Duration::hours(1)
                        }
                    }
                });

                (found, q_schedules.items.len())
            };

            if remainder > 0 || found - remainder > 0 {
                info!(
                    "{} schedule units remains. {} of unit(s) dropped.",
                    remainder,
                    found - remainder
                )
            } else {
                debug!(
                    "{} schedule units remains. {} of unit(s) dropped.",
                    remainder,
                    found - remainder
                )
            }

            for item in q_schedules.items.iter() {
                match item {
                    // ????????????????????????
                    Schedule {
                        is_active: true,
                        program:
                            Program {
                                duration: Some(length_msec),
                                ..
                            },
                        ..
                    } => {
                        //?????????????????????
                        let save_location = {
                            let candidate = match item.plan_id {
                                Some((id, PlanUnit::Word(_))) => format!("./word_{}/", id),
                                Some((id, PlanUnit::Series(_))) => format!("./series_{}/", id),
                                None => "./common/".to_string(),
                            };
                            if let Err(e) = std::fs::create_dir_all(&candidate) {
                                error!("Failed to create dir at {}.\n{}", &candidate, e);
                                continue;
                            }
                            std::fs::canonicalize(candidate)?
                        };

                        let task = RecordingTaskDescription {
                            program: item.program.clone(),
                            save_dir_location: save_location,
                        };

                        if is_in_the_recording_range(
                            // ????????????10????????????
                            (item.program.start_at - Duration::minutes(10)).into(),
                            item.program.start_at.into(),
                            Local::now(),
                        ) {
                            // Mirakurun??????????????????????????????
                            tx.send(RecordControlMessage::CreateOrUpdate(task))
                                .await
                                .unwrap();
                        } else if is_in_the_recording_range(
                            // ?????????
                            item.program.start_at.into(),
                            (item.program.start_at + Duration::milliseconds(*length_msec as i64))
                                .into(),
                            Local::now(),
                        ) {
                            // Mirakurun???????????????????????????????????????????????????????????????????????????
                            // ????????????????????????????????????
                            // ???????????????????????????DL???????????????????????????ex. ????????????????????????????????????????????????????????????
                            tx.send(RecordControlMessage::TryCreate(task))
                                .await
                                .unwrap();
                        }
                    }
                    Schedule {
                        is_active: true,
                        program: Program { duration: None, .. },
                        ..
                    } => {
                        error!("?????????????????????????????????duration??????")
                    }
                    _ => continue,
                }
            }
        }
        debug!("Scanning schedules completed. Now releasing q_schedules.");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

#[inline]
fn is_in_the_recording_range(
    left: DateTime<Local>,
    right: DateTime<Local>,
    value: DateTime<Local>,
) -> bool {
    assert!(left < right);
    (left < value) && (value < right)
}
