use std::path::PathBuf;

use crate::api::api_startup;
use ::mirakurun_client::models::Program;
use serde_derive::{Deserialize, Serialize};
use tokio::select;

use crate::context::Context;
use crate::epg_syncer::epg_sync_startup;
use crate::sched_trigger::sched_trigger_startup;

mod api;
mod context;
mod db_utils;
mod epg_syncer;
mod mirakurun_client;
mod recording_planner;
// mod recording_pool;
mod sched_trigger;

#[derive(Debug)]
pub enum RecordControlMessage {
    CreateOrUpdate(RecordingTaskDescription),
    TryCreate(RecordingTaskDescription),
    Remove(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingTaskDescription {
    pub program: Program,
    pub save_dir_location: PathBuf,
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let cx = Context::new();

    //Create Recording Queue Notifier
    let (rqn_tx, rqn_rx) = tokio::sync::mpsc::channel(100);
    select! {
        _ = api_startup(cx.clone()) => {},
        _ = sched_trigger_startup(cx.clone(), rqn_tx) => {},
        _ = epg_sync_startup(cx.clone()) => {}
    };
}
