use log::debug;
use meilisearch_sdk::errors::Error;
use meilisearch_sdk::tasks::Task;

use crate::db_utils::*;
use crate::epg_syncer::EpgSyncManager;
use crate::mirakurun_client::{
    fetch_programmes, fetch_services, ProgramsReturnType, ServicesReturnType,
};

pub(crate) struct RefreshDbResult {
    pub(crate) s: Task,
    pub(crate) p: Task,
}

impl EpgSyncManager {
    async fn fetch_epg(&self) -> (ServicesReturnType, ProgramsReturnType) {
        let p = fetch_programmes(&self.m_conf).await;
        let s = fetch_services(&self.m_conf).await;
        (s, p)
    }
    pub(crate) async fn refresh_db(&self, overwrite: bool) -> Result<RefreshDbResult, Error> {
        // Periodically updates the list of currently available channels, future programs.
        // This is triggered every 10 minutes.
        let initial_epg = self.fetch_epg().await;

        let s = replace_services_ranges(&self.index_services, &initial_epg.0.unwrap()).await?;
        debug!("{:?}", s);
        let p = if overwrite {
            replace_programs_ranges(&self.index_programs, &initial_epg.1.unwrap()).await?
        } else {
            push_programs_ranges(&self.index_programs, &initial_epg.1.unwrap()).await?
        };
        debug!("{:?}", p);

        Ok(RefreshDbResult { s, p })
    }
}
