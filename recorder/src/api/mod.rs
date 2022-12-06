use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::response::IntoResponse;
use axum::{
    response,
    routing::{delete, get, put},
    Router,
};
use log::info;
use ulid::Ulid;

use crate::context::Context;
use crate::db_utils::{get_all_programs, get_all_services, get_temporary_accessor, pull_program};
use crate::recording_planner::PlanUnit;
use crate::recording_pool::REC_POOL;
use crate::sched_trigger::Schedule;
use crate::RecordingTaskDescription;

pub(crate) async fn api_startup(cx: Arc<Context>) {
    let cx1 = cx.clone();
    let cx2 = cx.clone();
    let cx3 = cx.clone();
    let cx4 = cx.clone();
    let cx5 = cx.clone();
    let cx6 = cx.clone();
    let cx7 = cx.clone();

    let app = Router::new()
        .route(
            "/",
            get(move || async move {
                serde_json::to_string(&cx1.q_schedules.read().unwrap().items).unwrap()
            }),
        )
        .route(
            "/programs",
            get(move || async {
                let client = get_temporary_accessor(cx2);
                let res = get_all_programs(&client).await;
                match res {
                    Ok(res) => Ok(response::Json(res)),
                    Err(e) => Err(e.to_string().into_response()),
                }
            }),
        )
        .route(
            "/services",
            get(|| async {
                let client = get_temporary_accessor(cx3);
                let res = get_all_services(&client).await;
                match res {
                    Ok(res) => Ok(response::Json(res)),
                    Err(e) => Err(e.to_string().into_response()),
                }
            }),
        )
        .route(
            "/q/sched",
            get(move || async move {
                match cx4.q_schedules.read() {
                    Ok(res) => Ok(response::Json(res.items.clone())),
                    Err(e) => Err(e.to_string().into_response()),
                }
            }),
        )
        .route("/q/sched", delete(|p| delete_sched(cx5, p)))
        .route(
            "/q/recording",
            get(move || async move {
                match REC_POOL.read() {
                    Ok(res) => Ok(response::Json(
                        res.iter()
                            .cloned()
                            .collect::<Vec<RecordingTaskDescription>>(),
                    )),
                    Err(e) => Err(e.to_string().into_response()),
                }
            }),
        )
        .route(
            "/new/sched",
            put(move |p| async move { put_recording_schedule(cx6, p).await }),
        )
        .route(
            "/q/rules",
            get(move || async move {
                match cx7.q_rules.read() {
                    Ok(res) => Ok(response::Json(
                        res.iter()
                            .map(|f| (f.0.clone(), f.1.clone()))
                            .collect::<Vec<(Ulid, PlanUnit)>>(),
                    )),
                    Err(e) => Err(e.to_string().into_response()),
                }
            }),
        );

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("listening on {}", addr);
    let e = axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn put_recording_schedule(
    cx: Arc<Context>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<response::Json<Schedule>, String> {
    let program = {
        let client = get_temporary_accessor(&cx);
        // Check input
        let id = params
            .get("id")
            .ok_or("invalid query string\n")?
            .parse::<i64>()
            .map_err(|e| e.to_string())?;
        // Pull
        pull_program(&client, id)
            .await
            .or_else(|e| Err(e.to_string()))?
    };
    let s = Schedule {
        program,
        plan_id: None,
        is_active: true,
    };

    let items = &mut cx.q_schedules.write().unwrap().items;
    if items.iter().all(|f| f.program.id != s.program.id) {
        items.push(s.clone());
    }

    info!("Program {:?} (service_id={}, network_id={}, event_id={}) has been successfully added to sched_trigger.",
        &s.program.description,
        &s.program.service_id,
        &s.program.network_id,
        &s.program.event_id,
    );
    Ok(response::Json(s))
}

async fn delete_sched(
    cx: Arc<Context>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<(), String> {
    // Check input
    let id = params
        .get("id")
        .ok_or("invalid query string\n")?
        .parse::<i64>()
        .map_err(|e| e.to_string())?;

    // Delete
    cx.q_schedules
        .write()
        .unwrap()
        .items
        .retain(|f| f.program.id == id);

    Ok(())
}
