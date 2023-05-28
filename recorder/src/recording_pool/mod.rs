use std::sync::Arc;

use lockfree::map::Map;
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use tokio::sync::mpsc::Receiver;
use tokio::task::{AbortHandle, JoinSet};

use crate::context::Context;
use crate::recording_pool::recording_task::{RecExitType, RecTask};
use crate::{RecordControlMessage, RecordingTaskDescription};

mod recording_task;

pub(crate) static mut THREAD_POOL: Lazy<JoinSet<std::io::Result<RecExitType>>> =
    Lazy::new(JoinSet::new);
pub(crate) static REC_POOL: Lazy<Map<i32, RecordingTaskDescription>> = Lazy::new(Map::new);

pub(crate) async fn recording_pool_startup(
    cx: Arc<Context>,
    mut rx: Receiver<RecordControlMessage>,
) {
    // Process messages one after another
    loop {
        let received = rx.recv().await;

        if let Some(received) = received.as_ref() {
            debug!("Incoming RecordControlMessage:\n {:?}", received);
        }

        match received {
            Some(RecordControlMessage::CreateOrUpdate(info)) => {
                let view_title_name = info.program.name.clone().unwrap_or("untitled".to_string());

                let contains = REC_POOL
                    .iter()
                    .any(|v| *v.key() == info.program.event_id && v.val().program == info.program);
                if contains {
                    debug!(
                        "{}(id={}) is already being recorded, thus it's overwritten.",
                        view_title_name, info.program.id
                    );
                    REC_POOL.insert(info.program.event_id, info.clone());
                } else {
                    let _handle = match insert_task(cx.clone(), info.clone()).await {
                        Ok(h) => h,
                        Err(e) => {
                            error!("{}", e);
                            REC_POOL.remove(&info.program.event_id);
                            continue;
                        }
                    };
                }
                info!(
                    "CreateOrUpdate for {} successfully processed.",
                    info.program.id
                );
            }
            Some(RecordControlMessage::Remove(eid)) => {
                if REC_POOL.remove(&eid).is_some() {
                    info!("id: {} is removed from recording pool.", eid)
                }
            }
            Some(RecordControlMessage::TryCreate(info)) => {
                let view_title_name = info.program.name.clone().unwrap_or("untitled".to_string());

                let _kill_handle = {
                    let contains = REC_POOL.iter().any(|v| *v.key() == info.program.event_id);
                    if contains {
                        debug!(
                            "{}(id={}) is already being recorded, thus it's skipped.",
                            view_title_name, info.program.id
                        );
                        continue;
                    } else {
                        match insert_task(cx.clone(), info.clone()).await {
                            Ok(h) => h,
                            Err(e) => {
                                error!("{}", e);
                                REC_POOL.remove(&info.program.event_id);
                                continue;
                            }
                        }
                    }
                };
                info!("TryCreate for {} successfully processed.", info.program.id);
            }
            None => continue,
        };
    }
}

async fn insert_task(
    cx: Arc<Context>,
    info: RecordingTaskDescription,
) -> std::io::Result<AbortHandle> {
    let id = info.program.id;
    let eid = info.program.event_id;
    let task = RecTask::new(cx.mirakurun_base_uri.clone(), info).await?;

    info!(
        "id: {} is newly inserted into recording pool.",
        id
    );

    let logger = async move {
        let mut result = task.await;
        let info = REC_POOL.remove(&eid);
        match result {
            Ok(RecExitType::Aborted(_)) => warn!("[id={}] Aborted.", id),
            Ok(RecExitType::Success(_)) => info!("[id={}] Success.", id),
            Ok(RecExitType::Continue(nid, sid, eid, _size)) if info.is_some() => {
                // Event relay.
                let mut info = info.unwrap().val().clone();
                info.id_override = Some((nid, sid, eid));
                result = RecTask::new(cx.mirakurun_base_uri.clone(), info)
                    .await?
                    .await
            }
            Err(ref e) => {
                error!("[id={}] Unexpected exit.", id);
                error!("{:#?}", e);
            }
            _ => unreachable!(),
        }
        result
    };

    unsafe { Ok(THREAD_POOL.spawn(logger)) }
}
