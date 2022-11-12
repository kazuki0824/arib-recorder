use std::time::Duration;

use meilisearch_sdk::errors::Error;
use meilisearch_sdk::indexes::Index;
use meilisearch_sdk::tasks::Task;
use meilisearch_sdk::Client;
use mirakurun_client::models::{Program, Service};

use crate::context::Context;

pub(crate) fn get_temporary_accessor<C: AsRef<Context>>(cx: C) -> Client {
    // Initialize Meilisearch client
    Client::new(
        cx.as_ref().meilisearch_base_uri.clone(),
        cx.as_ref().meilisearch_api_key.clone(),
    )
}

pub async fn push_programs_ranges(index: &Index, data: &[Program]) -> Result<Task, Error> {
    index
        .add_or_update(data, Some("id"))
        .await?
        .wait_for_completion(&index.client, None, Some(Duration::from_secs(10)))
        .await
}

pub async fn push_services_ranges(index: &Index, data: &[Service]) -> Result<Task, Error> {
    index
        .add_or_update(data, Some("id"))
        .await?
        .wait_for_completion(&index.client, None, Some(Duration::from_secs(10)))
        .await
}

pub async fn pull_program(client: &Client, id: i64) -> Result<Program, Error> {
    client
        .get_index("_programs")
        .await?
        .get_document(&*id.to_string())
        .await
}

pub async fn pull_service(client: &Client, id: i64) -> Result<Service, Error> {
    client
        .get_index("_services")
        .await?
        .get_document(&*id.to_string())
        .await
}

pub async fn get_all_programs(client: &Client) -> Result<Vec<Program>, Error> {
    client
        .get_index("_programs")
        .await?
        .get_documents()
        .await
        .and_then(|f| Ok(f.results))
}

pub async fn get_all_services(client: &Client) -> Result<Vec<Service>, Error> {
    client
        .get_index("_services")
        .await?
        .get_documents()
        .await
        .and_then(|f| Ok(f.results))
}
