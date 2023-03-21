use std::path::PathBuf;

use ::mirakurun_client::models::Program;
use log::error;
use serde_derive::{Deserialize, Serialize};
use tokio::select;

use crate::context::Context;
use crate::{
    api::api_startup, epg_syncer::epg_sync_startup, recording_pool::recording_pool_startup,
    sched_trigger::sched_trigger_startup,
};

mod api;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingTaskDescription {
    pub program: Program,
    pub id_override: Option<(i64, i64, i64)>,
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
        Err(e) = sched_trigger_startup(cx.clone(), rqn_tx) => error!("{}", e),
        Err(e) = epg_sync_startup(cx.clone()) => error!("{}", e),
        _ = recording_pool_startup(cx.clone(), rqn_rx) => {},
        _ = tokio::signal::ctrl_c() => { println!("First signal: gracefully exitting...") }
    }
}
