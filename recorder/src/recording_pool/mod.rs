use std::path::PathBuf;
use mirakurun_client::models::Program;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct RecordingTaskDescription {
    pub program: Program,
    pub save_dir_location: PathBuf,
}
