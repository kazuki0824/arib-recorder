use log::warn;
use mirakurun_client::apis::channels_api::GetChannelsError;
use mirakurun_client::apis::configuration::Configuration;
use mirakurun_client::apis::programs_api::{get_programs, GetProgramsError};
use mirakurun_client::apis::services_api::{get_services, GetServicesError};
use mirakurun_client::apis::Error;
use mirakurun_client::models::{Channel, Program, Service};

pub type ChannelsReturnType = Result<Vec<Channel>, Error<GetChannelsError>>;
pub type ServicesReturnType = Result<Vec<Service>, Error<GetServicesError>>;
pub type ProgramsReturnType = Result<Vec<Program>, Error<GetProgramsError>>;

pub(crate) async fn fetch_services(c: &Configuration) -> ServicesReturnType {
    get_services(c, None, None, None, None, None, None).await
}

pub(crate) async fn fetch_programmes(c: &Configuration) -> ProgramsReturnType {
    get_programs(c, None, None, None).await
}

pub(crate) async fn get_service_from_program(c: &Configuration, p: &Program) -> Option<Service> {
    let result = get_services(
        c,
        Some(p.service_id),
        Some(p.network_id),
        None,
        None,
        None,
        None,
    )
    .await;
    match result {
        Ok(v) if v.len() > 0 => Some(v[0].clone()),
        Err(e) => {
            warn!("{}", e);
            None
        }
        _ => None,
    }
}
