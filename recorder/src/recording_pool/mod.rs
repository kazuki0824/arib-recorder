use std::sync::{Arc, RwLock};

use log::{debug, info};
use once_cell::sync::Lazy;
use tokio::sync::mpsc::Receiver;

use crate::context::Context;
use crate::RecordControlMessage;

mod pool;
mod recording_task;

pub(crate) static REC_POOL: Lazy<RwLock<pool::RecTaskQueue>> =
    Lazy::new(|| RwLock::new(pool::RecTaskQueue::new()));

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
                REC_POOL.write().unwrap().add(cx.clone(), info)
            }
            Some(RecordControlMessage::Remove(id)) => {
                if REC_POOL.write().unwrap().try_remove(&id) {
                    info!("id: {} is removed from recording pool.", id)
                }
            }
            Some(RecordControlMessage::TryCreate(info)) => {
                let id = info.program.id;
                let view_title_name = info.program.name.clone().unwrap_or("untitled".to_string());
                if REC_POOL.write().unwrap().try_add(cx.clone(), info) {
                    info!("A new program (id={}) has been added to recording queue because of an incoming message.", id)
                } else {
                    info!(
                        "{}(id={}) is already being recorded, thus it's skipped.",
                        view_title_name, id
                    )
                }
            }
            None => continue,
        };
    }
}
