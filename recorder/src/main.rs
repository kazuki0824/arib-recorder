use tokio::select;

use crate::context::Context;
use crate::epg_syncer::epg_sync_startup;
use crate::recording_pool::RecordingTaskDescription;
use crate::sched_trigger::sched_trigger_startup;

mod context;
mod db_utils;
mod epg_syncer;
mod mirakurun_client;
mod recording_planner;
mod recording_pool;
mod sched_trigger;

#[derive(Debug)]
pub enum RecordControlMessage {
    CreateOrUpdate(RecordingTaskDescription),
    TryCreate(RecordingTaskDescription),
    Remove(i64),
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let cx = Context::new();

    //Create Recording Queue Notifier
    let (rqn_tx, rqn_rx) = tokio::sync::mpsc::channel(100);
    select! {
        _ = sched_trigger_startup(cx.clone(), rqn_tx) => {},
        _ = epg_sync_startup(cx.clone()) => {}
    };
}
