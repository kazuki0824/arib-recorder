use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use log::{debug, error, info};
use meilisearch_sdk::indexes::Index;
use meilisearch_sdk::settings::Settings;
use meilisearch_sdk::Client;
use mirakurun_client::apis::configuration::Configuration;
use mirakurun_client::models::event::EventContent;

use crate::context::Context;
use crate::db_utils::{push_programs_ranges, push_services_ranges};

mod events_stream;
mod periodic_tasks;

pub(crate) async fn epg_sync_startup(
    cx: Arc<Context>,
) -> Result<(), meilisearch_sdk::errors::Error> {
    let tracker = EpgSyncManager::new(cx).await?;
    tracker.run().await
}

pub(crate) struct EpgSyncManager {
    m_conf: Configuration,
    search_client: Client,
    index_programs: Index,
    index_services: Index,
    cx: Arc<Context>,
}

impl EpgSyncManager {
    pub(crate) async fn new(cx: Arc<Context>) -> Result<Self, meilisearch_sdk::errors::Error> {
        let (m_url, db_url, api_key) = {
            (
                cx.mirakurun_base_uri.clone(),
                cx.meilisearch_base_uri.clone(),
                cx.meilisearch_api_key.clone(),
            )
        };

        // Initialize Mirakurun
        let mut m_conf = Configuration::new();
        m_conf.base_path = m_url;

        // Initialize Meilisearch client
        let search_client = Client::new(db_url, api_key);

        // Try to get the inner indices if the task succeeded
        let index_programs = match search_client.get_index("_programs").await {
            Ok(index) => index,
            Err(_) => {
                let task = search_client.create_index("_programs", Some("id")).await?;
                let task = task.wait_for_completion(&search_client, None, None).await?;
                task.try_make_index(&search_client).unwrap()
            }
        };
        let index_services = match search_client.get_index("_services").await {
            Ok(index) => index,
            Err(_) => {
                let task = search_client.create_index("_services", Some("id")).await?;
                let task = task.wait_for_completion(&search_client, None, None).await?;
                task.try_make_index(&search_client).unwrap()
            }
        };

        let s = Settings::new()
            .with_searchable_attributes(&["name", "extended"])
            .with_filterable_attributes(&["start_at", "genres"])
            .with_sortable_attributes(&["start_at"]);

        index_programs.set_settings(&s).await?;
        index_services.set_settings(&s).await?;

        Ok(Self {
            m_conf,
            search_client,
            index_programs,
            index_services,
            cx,
        })
    }

    pub(crate) async fn run(self) -> Result<(), meilisearch_sdk::errors::Error> {
        let periodic = async {
            const PERIODIC_EPG_UPDATE_SEC: u64 = 600;

            info!("Reclaiming EPG DB...");
            let res_refresh = self.refresh_db(true).await?;
            info!("Reclaiming has finished. {{}} of services / {{}} of programs are found.");

            info!(
                "Periodic EPG update is running every {} seconds.",
                PERIODIC_EPG_UPDATE_SEC
            );
            loop {
                tokio::time::sleep(Duration::from_secs(PERIODIC_EPG_UPDATE_SEC)).await;

                self.refresh_db(false).await?;
                info!("refresh_db() succeeded.");
            }
        };

        let event = async {
            const RECONNECTION_MAX: i64 = 10;

            tokio::time::sleep(Duration::from_secs(2)).await;
            let mut reconnection_counter = -1;
            loop {
                reconnection_counter += 1;
                if reconnection_counter >= RECONNECTION_MAX {
                    //TODO: Avoid excessive load
                    debug!("Reconnection limit reached.")
                }
                // Subscribe NDJSON here.
                // Store programs data into DB, and keep track of them using Mirakurun's Events API.
                let mut stream = match self.update_db_from_stream().await {
                    Ok(value) => value,
                    Err(e) => {
                        error!("{:#?}", e);
                        error!("Reconnecting to Events API again.");
                        continue;
                    }
                };

                // filter
                'inner: loop {
                    let next_str = match stream.next().await {
                        Some(Ok(line)) => {
                            if line.trim().eq_ignore_ascii_case("[") {
                                continue;
                            };
                            debug!("feed length = {}", line.len());
                            line
                        }
                        _ => continue,
                    };

                    match serde_json::from_str(&next_str) {
                        Ok(EventContent::Service(value)) => {
                            info!("Updating the service: {:?}", value);
                            match push_services_ranges(&self.index_services, &vec![value]).await {
                                Ok(_) => info!("Updates have been successfully applied."),
                                Err(e) => error!("{}", e),
                            }
                            continue;
                        }
                        Ok(EventContent::Program(value)) => {
                            match push_programs_ranges(&self.index_programs, &vec![value.clone()])
                                .await
                            {
                                Ok(_) => {
                                    // Update schedules
                                    {
                                        let mut q_schedules = self.cx.q_schedules.write().await;
                                        q_schedules.items.iter_mut().for_each(|mut f| {
                                            if value.id == f.program.id {
                                                info!("EIT[p/f] from Mirakurun. \n{:?}", &value);
                                                info!(
                                                    "Program Id={} in q_schedules has been overwritten.",
                                                    value.id
                                                );
                                                f.program = value.clone();
                                            }
                                        });
                                    }
                                }
                                Err(e) => error!("{}", e),
                            }
                            continue;
                        }
                        Ok(EventContent::Tuner(value)) => {
                            info!("Tuner configuration has been changed. {:?}", value)
                        }
                        Err(e) => {
                            debug!("In /events, {:?}", e);
                            break 'inner;
                        }
                    }
                }
                debug!("Trying to recover /events subscription.")
            }
        };

        tokio::select! {
            result = periodic => result,
            _ = event => Ok(()),
        }
    }
}
