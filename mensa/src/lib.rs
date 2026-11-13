use std::sync::{Arc, RwLock};

use anyhow::Result;
use axum::Router;
use tokio::sync::broadcast;

use crate::{
    constants::{CANTEEN_MAP, CANTEEN_MAP_INV},
    cronjobs::{start_canteen_cache_job, update_cache},
    db_operations::{check_or_create_db_tables, get_canteens_from_db},
    openmensa_funcs::init_openmensa_canteenlist,
    stuwe_request_funcs::invert_map,
};

mod constants;
mod cronjobs;
mod db_operations;
mod openmensa_funcs;
mod routes;
mod services;
mod stuwe_request_funcs;
mod types;

pub async fn app() -> Result<Router> {
    check_or_create_db_tables()?;
    let canteens = get_canteens_from_db().await?;
    *CANTEEN_MAP_INV.write().expect("canteen lock poisoned") = invert_map(&canteens);
    *CANTEEN_MAP.write().expect("canteen lock poisoned") = canteens;

    tokio::spawn(async {
        if let Err(error) = init_openmensa_canteenlist().await {
            log::error!("OpenMensa list could not be loaded: {error}");
        }
    });

    let (updates, _) = broadcast::channel(20);
    let scheduler_updates = updates.clone();
    tokio::spawn(async move {
        if let Err(error) = update_cache(None).await {
            log::error!("Initial canteen update failed: {error}");
        }
        let _scheduler =
            start_canteen_cache_job(scheduler_updates, Arc::new(RwLock::new(false))).await;
        std::future::pending::<()>().await;
    });

    Ok(routes::app(updates).await)
}
