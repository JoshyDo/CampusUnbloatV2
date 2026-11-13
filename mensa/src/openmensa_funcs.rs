use std::{collections::BTreeMap, env, vec};

use anyhow::Result;
use axum::{Json, extract::Path};
use chrono::NaiveDate;
use http::StatusCode;
use reqwest::Client;
use reqwest_middleware::ClientBuilder;
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use serde::Deserialize;

use crate::{
    constants::{OPENMENSA_ALL_CANTEENS, OPENMENSA_LIVE_CANTEENS},
    types::{Canteen, MealGroup, ResponseError, SingleMeal},
};

#[derive(Deserialize)]
struct Day {
    closed: bool,
}

pub async fn init_openmensa_canteenlist() -> Result<()> {
    if OPENMENSA_ALL_CANTEENS.get().is_some() {
        log::info!("OpenMensa list already initialized");
        return Ok(());
    }

    let reqwest_client = Client::builder().build().unwrap();
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
    let client = ClientBuilder::new(reqwest_client)
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build();

    let all_canteens: Vec<Canteen> = client
        .get("https://openmensa.org/api/v2/canteens")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    log::info!("got {} canteens", all_canteens.len());
    OPENMENSA_ALL_CANTEENS.set(all_canteens.clone()).unwrap();

    if env::var_os("FILTER_OPENMENSA").is_none() {
        return Ok(());
    };

    log::info!("Filtering OpenMensa canteen list, this might take a while");
    let mut canteens_with_days = vec![];

    for (iteration, canteen) in all_canteens.iter().enumerate() {
        if iteration % 50 == 0 {
            log::info!("{}%...", (iteration * 100) / all_canteens.len());
        }

        let days: Vec<Day> = client
            .get(format!(
                "https://openmensa.org/api/v2/canteens/{}/days",
                canteen.id
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        if !days.is_empty() && !days.iter().all(|day| day.closed) {
            canteens_with_days.push(canteen.clone());
        }
    }

    log::info!(
        "OpenMensa filtering done, {} canteens remain",
        canteens_with_days.len()
    );

    OPENMENSA_LIVE_CANTEENS.set(canteens_with_days).unwrap();
    Ok(())
}

pub async fn get_openmensa_canteens() -> Result<Json<Vec<Canteen>>, StatusCode> {
    if let Some(list) = OPENMENSA_LIVE_CANTEENS.get() {
        Ok(Json(list.clone()))
    } else if let Some(list) = OPENMENSA_ALL_CANTEENS.get() {
        log::warn!("Not filtering OpenMensa list, consider Env FILTER_OPENMENSA=y");
        Ok(Json(list.clone()))
    } else {
        log::warn!("OpenMensa list is not available");
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}

#[derive(Deserialize)]
struct OpenMeal {
    category: String,
    name: String,
    #[serde(default)]
    notes: Vec<String>,
    prices: OpenPrices,
}

#[derive(Deserialize)]
struct OpenPrices {
    students: Option<f64>,
    employees: Option<f64>,
    pupils: Option<f64>,
    others: Option<f64>,
}

pub async fn get_openmensa_meals(
    Path((canteen_id, date)): Path<(u32, String)>,
) -> Result<Json<Vec<MealGroup>>, ResponseError> {
    let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|_| ResponseError {
        message: "Invalid date format".to_string(),
        status_code: StatusCode::BAD_REQUEST,
    })?;
    let url = format!("https://openmensa.org/api/v2/canteens/{canteen_id}/days/{date}/meals");
    let response = Client::new().get(url).send().await.map_err(|error| {
        log::error!("OpenMensa request failed: {error}");
        ResponseError {
            message: "OpenMensa ist nicht erreichbar".to_string(),
            status_code: StatusCode::BAD_GATEWAY,
        }
    })?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(Json(Vec::new()));
    }
    let response = response.error_for_status().map_err(|error| {
        log::error!("OpenMensa returned an error: {error}");
        ResponseError {
            message: "OpenMensa hat einen Fehler gemeldet".to_string(),
            status_code: StatusCode::BAD_GATEWAY,
        }
    })?;
    let meals: Vec<OpenMeal> = response.json().await.map_err(|error| {
        log::error!("Invalid OpenMensa response: {error}");
        ResponseError {
            message: "OpenMensa hat ungültige Daten geliefert".to_string(),
            status_code: StatusCode::BAD_GATEWAY,
        }
    })?;

    let mut groups: BTreeMap<String, Vec<SingleMeal>> = BTreeMap::new();
    for meal in meals {
        let prices = [
            meal.prices.students,
            meal.prices.employees,
            meal.prices.pupils,
            meal.prices.others,
        ]
        .into_iter()
        .flatten()
        .map(|price| format!("{price:.2} €").replace('.', ","))
        .collect::<Vec<_>>()
        .join(" / ");
        groups.entry(meal.category).or_default().push(SingleMeal {
            name: meal.name,
            additional_ingredients: meal.notes,
            allergens: None,
            variations: None,
            price: prices,
        });
    }
    Ok(Json(
        groups
            .into_iter()
            .map(|(meal_type, sub_meals)| MealGroup {
                meal_type,
                sub_meals,
            })
            .collect(),
    ))
}
