use campusunbloat_shared::{Canteen, MealGroup};
use chrono::{Datelike, Duration, Local, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use web_sys::{HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

use crate::api::{self, Remote};

use super::RefreshProps;

const PREFERENCE_KEY: &str = "campusunbloat-v2-openmensa-canteen";
const DEFAULT_CANTEEN: u32 = 69;

#[derive(Serialize, Deserialize)]
struct Preference {
    id: u32,
}

fn read_preference() -> Option<Preference> {
    let storage = web_sys::window().and_then(|window| window.local_storage().ok().flatten())?;
    match storage.get_item(PREFERENCE_KEY) {
        Ok(Some(value)) => match serde_json::from_str(&value) {
            Ok(preference) => Some(preference),
            Err(error) => {
                web_sys::console::warn_1(&format!("Ungültige Mensa-Einstellung: {error}").into());
                None
            }
        },
        Ok(None) => None,
        Err(error) => {
            web_sys::console::warn_1(&error);
            None
        }
    }
}

fn save_preference(id: u32) {
    let storage = match web_sys::window().and_then(|window| window.local_storage().ok().flatten()) {
        Some(storage) => storage,
        None => {
            web_sys::console::warn_1(&"Mensa-Einstellung konnte nicht gespeichert werden.".into());
            return;
        }
    };
    match serde_json::to_string(&Preference { id }) {
        Ok(value) => {
            if let Err(error) = storage.set_item(PREFERENCE_KEY, &value) {
                web_sys::console::warn_1(&error);
            }
        }
        Err(error) => {
            web_sys::console::warn_1(&format!("Ungültige Mensa-Einstellung: {error}").into())
        }
    }
}

fn next_weekday(date: NaiveDate, direction: i64) -> NaiveDate {
    let mut next = date + Duration::days(direction);
    while matches!(next.weekday(), Weekday::Sat | Weekday::Sun) {
        next += Duration::days(direction);
    }
    next
}

fn first_day() -> NaiveDate {
    Local::now().date_naive()
}

fn weekday_name(date: NaiveDate) -> &'static str {
    match date.weekday() {
        Weekday::Mon => "Montag",
        Weekday::Tue => "Dienstag",
        Weekday::Wed => "Mittwoch",
        Weekday::Thu => "Donnerstag",
        Weekday::Fri => "Freitag",
        Weekday::Sat => "Samstag",
        Weekday::Sun => "Sonntag",
    }
}

#[function_component(Mensa)]
pub fn mensa(props: &RefreshProps) -> Html {
    let saved = read_preference();
    let selected = use_state(|| {
        saved
            .as_ref()
            .map(|value| value.id)
            .unwrap_or(DEFAULT_CANTEEN)
    });
    let search = use_state(String::new);
    let date = use_state(first_day);
    let canteens = api::use_remote::<Vec<Canteen>>(
        "/api/mensa/openmensacanteens".to_string(),
        props.generation,
    );
    {
        let selected = selected.clone();
        use_effect_with((*canteens).clone(), move |canteens| {
            if let Remote::Ready(items) = canteens
                && !items.iter().any(|canteen| canteen.id == *selected)
            {
                if let Some(first) = items.first() {
                    selected.set(first.id);
                    save_preference(first.id);
                } else {
                    selected.set(0);
                }
            }
            || ()
        });
    }
    let change_canteen = {
        let selected = selected.clone();
        Callback::from(move |event: Event| {
            let value = event.target_unchecked_into::<HtmlSelectElement>().value();
            match value.parse::<u32>() {
                Ok(id) => {
                    selected.set(id);
                    save_preference(id);
                }
                Err(error) => {
                    web_sys::console::error_1(&format!("Ungültige Mensa: {error}").into())
                }
            }
        })
    };
    let change_search = {
        let search = search.clone();
        Callback::from(move |event: InputEvent| {
            search.set(event.target_unchecked_into::<HtmlInputElement>().value());
        })
    };
    let previous = {
        let date = date.clone();
        Callback::from(move |_| date.set(next_weekday(*date, -1)))
    };
    let next = {
        let date = date.clone();
        Callback::from(move |_| date.set(next_weekday(*date, 1)))
    };
    let today = {
        let date = date.clone();
        Callback::from(move |_| date.set(first_day()))
    };

    html! {
        <section class="card mensa-card">
            <div class="card-heading">
                <div>
                    <span class="eyebrow">{"SPEISEPLAN"}</span>
                    <h2>{"Mensa"}</h2>
                </div>
                <span class="card-symbol" aria-hidden="true">{"04"}</span>
            </div>
            <div class="mensa-fields">
                <label for="mensa-search">{"Mensa suchen"}</label>
                <input id="mensa-search" type="search" value={(*search).clone()} oninput={change_search} placeholder="Name eingeben" />
                {match &*canteens {
                    Remote::Loading => html! { <p class="muted">{"Mensen werden geladen …"}</p> },
                    Remote::Failed(error) => super::failure(error),
                    Remote::Ready(items) if items.is_empty() => html! {
                        <p class="muted">{"Keine Mensa-Daten verfügbar. Bitte später aktualisieren."}</p>
                    },
                    Remote::Ready(items) => {
                        let query = search.to_lowercase();
                        html! {
                            <>
                                <label for="mensa-canteen">{"Mensa"}</label>
                                <select id="mensa-canteen" onchange={change_canteen}>
                                    {for items.iter()
                                        .filter(|item| item.id == *selected || item.name.to_lowercase().contains(&query))
                                        .map(|item| html! { <option value={item.id.to_string()} selected={item.id == *selected}>{&item.name}</option> })}
                                </select>
                                <div class="date-nav">
                                    <button class="date-arrow" onclick={previous} aria-label="Vorheriger Werktag">{"‹"}</button>
                                    <button class="date-label" onclick={today} title="Zu heute springen">
                                        <strong>{weekday_name(*date)}</strong>
                                        <span>{date.format("%d.%m.%Y").to_string()}</span>
                                    </button>
                                    <button class="date-arrow" onclick={next} aria-label="Nächster Werktag">{"›"}</button>
                                </div>
                                {if *selected != 0 && items.iter().any(|item| item.id == *selected) {
                                    html! { <Meals id={*selected} date={*date} generation={props.generation} /> }
                                } else { html! {} }}
                            </>
                        }
                    },
                }}
            </div>
        </section>
    }
}

#[derive(Properties, PartialEq)]
struct MealsProps {
    id: u32,
    date: NaiveDate,
    generation: u64,
}

#[function_component(Meals)]
fn meals(props: &MealsProps) -> Html {
    let path = format!(
        "/api/mensa/openmensacanteens/{}/days/{}",
        props.id,
        props.date.format("%Y-%m-%d")
    );
    let meals = api::use_remote::<Vec<MealGroup>>(path, props.generation);
    html! {
        <div class="meal-list">
            {match &*meals {
                Remote::Loading => html! { <p class="muted">{"Speiseplan wird geladen …"}</p> },
                Remote::Failed(error) => super::failure(error),
                Remote::Ready(groups) if groups.is_empty() => html! { <p class="muted">{"Für diesen Tag sind keine Gerichte verfügbar."}</p> },
                Remote::Ready(groups) => html! {
                    {for groups.iter().map(|group| html! {
                        <div class="meal-group">
                            <h3>{&group.meal_type}</h3>
                            {for group.sub_meals.iter().map(|meal| html! {
                                <div class="meal">
                                    <div>
                                        <strong>{&meal.name}</strong>
                                        {if !meal.additional_ingredients.is_empty() {
                                            html! { <small>{meal.additional_ingredients.join(", ")}</small> }
                                        } else { html! {} }}
                                        {meal.allergens.as_ref().filter(|value| !value.is_empty()).map(|value| html! { <small>{value}</small> }).unwrap_or_default()}
                                        {meal.variations.as_ref().map(|variations| html! {
                                            {for variations.iter().map(|variation| html! { <small>{&variation.name}</small> })}
                                        }).unwrap_or_default()}
                                    </div>
                                    <span>{&meal.price}</span>
                                </div>
                            })}
                        </div>
                    })}
                },
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekdays_skip_the_weekend_in_both_directions() {
        let friday = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        let monday = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        assert_eq!(next_weekday(friday, 1), monday);
        assert_eq!(next_weekday(monday, -1), friday);
    }
}
