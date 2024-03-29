use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use structopt::StructOpt;

use crate::recording_planner::RulesResolverBase;
use crate::sched_trigger::SchedQueue;

#[derive(Debug, StructOpt)]
#[structopt(name = "meister", about = "An example of StructOpt usage.")]
struct Opt {
    #[structopt(default_value = "http://localhost:40772/api")]
    mirakurun_base_uri: String,
    #[structopt(default_value = "http://localhost:7700/")]
    meilisearch_base_uri: String,
    #[structopt(short)]
    meilisearch_api_key: Option<String>,
    #[structopt(short)]
    schedules_path: Option<PathBuf>,
    #[structopt(short)]
    rules_path: Option<PathBuf>,
}

pub(crate) struct Context {
    pub(crate) mirakurun_base_uri: String,
    pub(crate) meilisearch_base_uri: String,
    pub(crate) meilisearch_api_key: String,
    pub(crate) q_rules: RwLock<RulesResolverBase>,
    pub(crate) q_schedules: RwLock<SchedQueue>,
}

impl Context {
    pub(crate) fn new() -> Arc<Self> {
        let opt = Opt::from_args();

        let meilisearch_api_key = { opt.meilisearch_api_key.unwrap_or("masterKey".to_string()) };

        Arc::new(Self {
            mirakurun_base_uri: opt.mirakurun_base_uri,
            meilisearch_base_uri: opt.meilisearch_base_uri,
            meilisearch_api_key,
            q_rules: RwLock::new(RulesResolverBase::new(opt.rules_path).unwrap()),
            q_schedules: RwLock::new(SchedQueue::new(opt.schedules_path).unwrap()),
        })
    }
}
