use std::{cell::Cell, rc::Rc};

use campusunbloat_shared::ApiError;
use gloo_net::http::{Request, Response};
use serde::{Serialize, de::DeserializeOwned};
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub struct ApiFailure {
    pub status: Option<u16>,
    pub message: String,
}

impl ApiFailure {
    fn network(error: impl std::fmt::Display) -> Self {
        Self {
            status: None,
            message: format!("Der lokale Server ist nicht erreichbar: {error}"),
        }
    }
}

async fn error_from_response(response: Response) -> ApiFailure {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<ApiError>(&text)
        .map(|error| error.error)
        .ok()
        .or_else(|| {
            let trimmed = text.trim();
            (!trimmed.is_empty() && !trimmed.starts_with('<'))
                .then(|| trimmed.chars().take(240).collect())
        })
        .unwrap_or_else(|| format!("Die Anfrage ist fehlgeschlagen (HTTP {status})."));
    ApiFailure {
        status: Some(status),
        message,
    }
}

pub async fn get<T: DeserializeOwned>(path: &str) -> Result<T, ApiFailure> {
    let response = Request::get(path)
        .send()
        .await
        .map_err(ApiFailure::network)?;
    if !response.ok() {
        return Err(error_from_response(response).await);
    }
    response.json().await.map_err(|error| ApiFailure {
        status: None,
        message: format!("Ungültige Antwort vom Server: {error}"),
    })
}

pub async fn post<T: DeserializeOwned, B: Serialize>(
    path: &str,
    body: &B,
) -> Result<T, ApiFailure> {
    let request = Request::post(path)
        .json(body)
        .map_err(ApiFailure::network)?;
    let response = request.send().await.map_err(ApiFailure::network)?;
    if !response.ok() {
        return Err(error_from_response(response).await);
    }
    response.json().await.map_err(|error| ApiFailure {
        status: None,
        message: format!("Ungültige Antwort vom Server: {error}"),
    })
}

pub async fn logout() -> Result<(), ApiFailure> {
    let response = Request::post("/api/logout")
        .send()
        .await
        .map_err(ApiFailure::network)?;
    if response.ok() {
        Ok(())
    } else {
        Err(error_from_response(response).await)
    }
}

#[derive(Clone, PartialEq)]
pub enum Remote<T> {
    Loading,
    Ready(T),
    Failed(ApiFailure),
}

struct Snapshot<T> {
    path: String,
    generation: u64,
    result: Rc<Remote<T>>,
}

#[hook]
#[allow(clippy::multiple_bound_locations)]
pub fn use_remote<T: DeserializeOwned + 'static>(path: String, generation: u64) -> Rc<Remote<T>> {
    let initial_path = path.clone();
    let result = use_state(move || Snapshot {
        path: initial_path,
        generation,
        result: Rc::new(Remote::Loading),
    });
    {
        let result = result.clone();
        use_effect_with((path.clone(), generation), move |(path, generation)| {
            let request_path = path.clone();
            let request_generation = *generation;
            result.set(Snapshot {
                path: request_path.clone(),
                generation: request_generation,
                result: Rc::new(Remote::Loading),
            });
            let active = Rc::new(Cell::new(true));
            let request_active = active.clone();
            spawn_local(async move {
                let response = get(&request_path).await;
                if request_active.get() {
                    result.set(Snapshot {
                        path: request_path,
                        generation: request_generation,
                        result: Rc::new(match response {
                            Ok(value) => Remote::Ready(value),
                            Err(error) => Remote::Failed(error),
                        }),
                    });
                }
            });
            move || active.set(false)
        });
    }
    if result.path == path && result.generation == generation {
        result.result.clone()
    } else {
        Rc::new(Remote::Loading)
    }
}
