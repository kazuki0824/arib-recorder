use futures_util::stream::TryStreamExt;
use mirakurun_client::apis::events_api::{get_events_stream, GetEventsStreamError};
use mirakurun_client::apis::Error;
use tokio::io::AsyncBufReadExt;
use tokio_stream::wrappers::LinesStream;
use tokio_stream::Stream;
use tokio_util::io::StreamReader;

use crate::epg_syncer::EpgSyncManager;

impl EpgSyncManager {
    pub(crate) async fn update_db_from_stream(
        &self,
    ) -> Result<impl Stream<Item = Result<String, std::io::Error>>, Error<GetEventsStreamError>>
    {
        // subscribe_to_events_api
        Ok(LinesStream::new(
            StreamReader::new(
                get_events_stream(&self.m_conf, None, None)
                    .await?
                    .bytes_stream()
                    .map_err(|e: mirakurun_client::Error| {
                        std::io::Error::new(std::io::ErrorKind::Other, e)
                    }),
            )
            .lines(),
        ))
    }
}
