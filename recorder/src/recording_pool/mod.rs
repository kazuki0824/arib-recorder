use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use log::{debug, info};
use once_cell::sync::Lazy;
use tokio::sync::mpsc::Receiver;

use crate::context::Context;
use crate::recording_pool::recording_task::RecTask;
use crate::{RecordControlMessage, RecordingTaskDescription};

mod recording_task;

pub(crate) static REC_POOL: Lazy<RwLock<HashMap<i64, RecordingTaskDescription>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

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
                let task = RecTask::new(&cx.mirakurun_base_uri, info.program.id);
                REC_POOL.write().unwrap().insert(info.program.id, info);
            }
            Some(RecordControlMessage::Remove(id)) => {
                if REC_POOL.write().unwrap().remove(&id).is_some() {
                    info!("id: {} is removed from recording pool.", id)
                }
            }
            Some(RecordControlMessage::TryCreate(info)) => {
                let view_title_name = info.program.name.clone().unwrap_or("untitled".to_string());

                let mut pool_ref = REC_POOL.write().unwrap();
                if !pool_ref.contains_key(&info.program.id) {
                    let id = info.program.id;
                    let task = RecTask::new(&cx.mirakurun_base_uri, id);
                    pool_ref.insert(info.program.id, info);
                    info!("A new program (id={}) has been added to recording queue because of an incoming message.", id)
                } else {
                    info!(
                        "{}(id={}) is already being recorded, thus it's skipped.",
                        view_title_name, info.program.id
                    )
                };
            }
            None => continue,
        };
    }
}
