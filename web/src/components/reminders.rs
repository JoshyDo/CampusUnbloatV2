use campusunbloat_shared::{CampusReminders, ExamEntry, OptModules};
use chrono::NaiveDate;
use yew::prelude::*;

use crate::api::{self, Remote};

use super::RefreshProps;

fn readable_date(value: &str) -> String {
    NaiveDate::parse_from_str(value, "%Y%m%d")
        .map(|date| date.format("%d.%m.%Y").to_string())
        .unwrap_or_else(|_| value.to_string())
}

const SEEN_KEY: &str = "seen_exam_offers";

fn offer_key(exam: &ExamEntry, offer_no: &str) -> String {
    format!(
        "{}/{}/{}/{}",
        exam.sm_tlst_objid, exam.year, exam.period, offer_no
    )
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn load_seen() -> Vec<String> {
    storage()
        .and_then(|store| store.get_item(SEEN_KEY).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_seen(keys: &[String]) {
    if let (Some(store), Ok(raw)) = (storage(), serde_json::to_string(keys)) {
        let _ = store.set_item(SEEN_KEY, &raw);
    }
}

#[function_component(Reminders)]
pub fn reminders(props: &RefreshProps) -> Html {
    let reminders =
        api::use_remote::<CampusReminders>("/api/reminders".to_string(), props.generation);
    let open_exams =
        api::use_remote::<Vec<ExamEntry>>("/api/exams/open".to_string(), props.generation);
    let opt_modules =
        api::use_remote::<OptModules>("/api/opt-modules".to_string(), props.generation);
    let seen = use_state(load_seen);
    let open = use_state(|| false);
    let toggle = {
        let open = open.clone();
        Callback::from(move |_| open.set(!*open))
    };
    let new_offers: Vec<(String, String, String)> = match &*open_exams {
        Remote::Ready(exams) => exams
            .iter()
            .flat_map(|exam| {
                exam.offers.iter().map(move |offer| {
                    let detail = [offer.date.as_str(), offer.time.as_str()]
                        .iter()
                        .filter(|part| !part.is_empty())
                        .copied()
                        .collect::<Vec<_>>()
                        .join(" · ");
                    (offer_key(exam, &offer.offer_no), exam.name.clone(), detail)
                })
            })
            .collect(),
        _ => Vec::new(),
    };
    let new_offers: Vec<(String, String, String)> = new_offers
        .into_iter()
        .chain(match &*opt_modules {
            Remote::Ready(data) => data
                .periods
                .iter()
                .filter(|period| period.open)
                .flat_map(|period| {
                    period
                        .modules
                        .iter()
                        .filter(|module| module.can_register)
                        .map(move |module| {
                            (
                                format!("opt/{}/{}", period.title, module.title),
                                module.title.clone(),
                                format!("Wahlpflichtmodul · {}", period.title),
                            )
                        })
                })
                .collect(),
            _ => Vec::new(),
        })
        .filter(|(key, _, _)| !seen.contains(key))
        .collect();
    let mark_read = {
        let seen = seen.clone();
        let keys: Vec<String> = new_offers.iter().map(|(key, _, _)| key.clone()).collect();
        Callback::from(move |_: MouseEvent| {
            let mut all = (*seen).clone();
            all.extend(keys.iter().cloned());
            save_seen(&all);
            seen.set(all);
        })
    };
    let count = new_offers.len()
        + match &*reminders {
            Remote::Ready(value) => value.latest.len() + value.upcoming.len(),
            _ => 0,
        };

    html! {
        <div class="notification-wrap">
            <button class="button secondary" onclick={toggle.clone()} aria-expanded={open.to_string()}>
                {"Benachrichtigungen"}
                {if count > 0 { html! { <span class="count">{count}</span> } } else { html! {} }}
            </button>
            {if *open {
                html! {
                    <section class="notification-panel" role="dialog" aria-label="Benachrichtigungen">
                        <div class="card-heading">
                            <h2>{"Benachrichtigungen"}</h2>
                            <button class="icon-button" onclick={toggle} aria-label="Schließen">{"×"}</button>
                        </div>
                        {if !new_offers.is_empty() { html! {
                            <div class="notification-group">
                                <h3>{"Neue Angebote"}</h3>
                                {for new_offers.iter().map(|(_, name, detail)| html! {
                                    <div class="notification-item">
                                        <strong>{name}</strong>
                                        <small>{detail}</small>
                                    </div>
                                })}
                                <button class="button ghost compact" onclick={mark_read}>{"Als gelesen markieren"}</button>
                            </div>
                        }} else { html! {} }}
                        {match &*reminders {
                            Remote::Loading => html! { <p class="muted">{"Benachrichtigungen werden geladen …"}</p> },
                            Remote::Failed(error) => super::failure(error),
                            Remote::Ready(data) => html! {
                                <>
                                    {if data.latest.is_empty() && data.upcoming.is_empty() {
                                        html! { <p class="muted">{"Keine neuen Benachrichtigungen."}</p> }
                                    } else { html! {} }}
                                    {if !data.upcoming.is_empty() { html! {
                                        <div class="notification-group">
                                            <h3>{"Anstehende Prüfungen"}</h3>
                                            {for data.upcoming.iter().map(|exam| html! {
                                                <div class="notification-item">
                                                    <strong>{if exam.sm_stext.is_empty() { &exam.sm_short } else { &exam.sm_stext }}</strong>
                                                    <small>{format!("{} · {}", readable_date(&exam.evdat), exam.beguz.chars().take(5).collect::<String>())}</small>
                                                </div>
                                            })}
                                        </div>
                                    }} else { html! {} }}
                                    {if !data.latest.is_empty() { html! {
                                        <div class="notification-group">
                                            <h3>{"Aktuelle Ergebnisse"}</h3>
                                            {for data.latest.iter().map(|grade| html! {
                                                <div class="notification-item">
                                                    <strong>{&grade.awobject}</strong>
                                                    <small>{format!("{} · Note {}", readable_date(&grade.agrdate), grade.gradesymbol)}</small>
                                                </div>
                                            })}
                                        </div>
                                    }} else { html! {} }}
                                </>
                            },
                        }}
                    </section>
                }
            } else { html! {} }}
        </div>
    }
}
