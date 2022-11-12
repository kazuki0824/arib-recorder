use std::collections::HashMap;

use mirakurun_client::models::related_item::Type;
use mirakurun_client::models::Program;

use crate::epg_syncer::EpgSyncManager;

impl EpgSyncManager {
    async fn get_reverse_event_relay(p: &Vec<Program>) -> HashMap<i32, i32> {
        let mut table = HashMap::new();
        for elem in p {
            if let Some(ref rels) = elem.related_items {
                for r in rels {
                    if let Some(Type::Relay) = r.r#type {
                        table.insert(r.event_id.unwrap(), elem.event_id);
                    }
                }
            }
        }
        table
    }
}
