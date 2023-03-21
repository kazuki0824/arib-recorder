use std::sync::Arc;

use chrono::{DateTime, Duration, Local};
use log::info;
use mirakurun_client::models::Program;
use tonic::transport::Server;
use tonic::Code;
use ulid::Ulid;

use crate::api::grpc_page::{schedule_server::*, search_server::*, *};
use crate::context::Context;
use crate::db_utils::{get_temporary_db_accessor, perform_search_query, pull_program};
use crate::recording_planner::PlanUnit;
use crate::recording_pool::REC_POOL;
use crate::sched_trigger::Schedule;
use crate::RecordingTaskDescription;

pub(crate) async fn api_startup(cx: Arc<Context>) -> Result<(), Box<dyn std::error::Error>> {
    let addr = "127.0.0.1:50051".parse()?;

    Server::builder()
        .add_service(MyGrpcScheduleHandler::new(cx.clone()))
        .add_service(MyGrpcSearchHandler::new(cx.clone()))
        .serve(addr)
        .await?;

    Ok(())
}

pub mod grpc_page {
    tonic::include_proto!("page");
}

/* Schedule */
pub struct MyGrpcScheduleHandler {
    cx: Arc<Context>,
}
impl MyGrpcScheduleHandler {
    fn new(cx: Arc<Context>) -> ScheduleServer<Self> {
        ScheduleServer::new(Self { cx })
    }
}
/* Search */
pub struct MyGrpcSearchHandler {
    cx: Arc<Context>,
}
impl MyGrpcSearchHandler {
    fn new(cx: Arc<Context>) -> SearchServer<Self> {
        SearchServer::new(Self { cx })
    }
}

/* Impl */
#[tonic::async_trait]
impl grpc_page::search_server::Search for MyGrpcSearchHandler {
    async fn get_current_contents(
        &self,
        request: tonic::Request<()>,
    ) -> Result<tonic::Response<SearchResult>, tonic::Status> {
        let client = get_temporary_db_accessor(&self.cx);

        let now = Local::now();
        let (start, end) = (now - Duration::hours(1), now + Duration::hours(6));
        let filter = format!(
            "start_at >= {} AND start_at < {}",
            start.timestamp() * 1000,
            end.timestamp() * 1000
        );

        let results = perform_search_query::<Program>(&client, "_programs", "", &filter).await;

        match results {
            Ok(results) => {
                let items = results
                    .hits
                    .iter()
                    .map(|raw_result| ProgramId {
                        value: raw_result.result.id,
                    })
                    .collect::<Vec<ProgramId>>();

                Ok(tonic::Response::new(SearchResult { items }))
            }
            Err(e) => Err(tonic::Status::new(Code::InvalidArgument, e.to_string())),
        }
    }
    async fn simple_search_by_name(
        &self,
        request: tonic::Request<String>,
    ) -> Result<tonic::Response<SearchResult>, tonic::Status> {
        let client = get_temporary_db_accessor(&self.cx);

        let query = request.into_inner();
        let results = perform_search_query::<Program>(&client, "_programs", &query, "").await;

        match results {
            Ok(results) => {
                let items = results
                    .hits
                    .iter()
                    .map(|raw_result| ProgramId {
                        value: raw_result.result.id,
                    })
                    .collect::<Vec<ProgramId>>();

                Ok(tonic::Response::new(SearchResult { items }))
            }
            Err(e) => Err(tonic::Status::new(Code::InvalidArgument, e.to_string())),
        }
    }
}

#[tonic::async_trait]
impl grpc_page::schedule_server::Schedule for MyGrpcScheduleHandler {
    async fn create(
        &self,
        request: tonic::Request<ProgramId>,
    ) -> Result<tonic::Response<bool>, tonic::Status> {
        let program = {
            let client = get_temporary_db_accessor(&self.cx);
            // Check input
            let id = request.into_inner().value;
            // Pull
            pull_program(&client, id)
                .await
                .or_else(|e| Err(tonic::Status::new(tonic::Code::Internal, e.to_string())))?
        };
        let s = Schedule {
            program,
            plan_id: None,
            is_active: true,
        };

        let items = &mut self.cx.q_schedules.write().await.items;
        if items.iter().all(|f| f.program.id != s.program.id) {
            items.push(s.clone());
            info!("Program {:?} (service_id={}, network_id={}, event_id={}) has been successfully added to sched_trigger.",
                &s.program.description,
                &s.program.service_id,
                &s.program.network_id,
                &s.program.event_id,
            );
            Ok(tonic::Response::new(true))
        } else {
            Ok(tonic::Response::new(true))
        }
    }
    async fn list_all(
        &self,
        request: tonic::Request<()>,
    ) -> Result<tonic::Response<AllSchedules>, tonic::Status> {
        let items = self
            .cx
            .q_schedules
            .read()
            .await
            .items
            .iter()
            .map(|f| {
                let id = Some(ProgramId {
                    value: f.program.id,
                });
                let start_at: DateTime<Local> = DateTime::from(f.program.start_at);

                let details = Some(ScheduleDetails {
                    name: f.program.name.clone().unwrap(),
                    start_at_human: start_at.to_rfc2822(),
                    duration: f.program.duration,
                    end_at_human: f
                        .program
                        .duration
                        .map(|d| (start_at + Duration::milliseconds(d as i64)).to_rfc2822()),
                });

                ScheduleConfiguration { id, details }
            })
            .collect::<Vec<ScheduleConfiguration>>();
        Ok(tonic::Response::new(AllSchedules { items }))
    }
    async fn remove(
        &self,
        request: tonic::Request<ProgramId>,
    ) -> Result<tonic::Response<()>, tonic::Status> {
        // Delete
        let id = request.into_inner().value;
        self.cx
            .q_schedules
            .write()
            .await
            .items
            .retain(|f| f.program.id == id);
        Ok(tonic::Response::new(()))
    }
}
