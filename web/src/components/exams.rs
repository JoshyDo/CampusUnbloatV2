use campusunbloat_shared::{ExamActionRequest, ExamActionResult, ExamEntry, ExamOffer, ExamPeriod};
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlSelectElement;
use yew::prelude::*;

use crate::api::{self, Remote};

use super::RefreshProps;
use super::receipt::{self, Receipt};

#[derive(Clone, PartialEq)]
struct Done {
    message: String,
    receipt: Receipt,
}

type Outcome = Result<Done, String>;

#[derive(Properties, PartialEq)]
struct OfferProps {
    exam: ExamEntry,
    offer: ExamOffer,
    on_result: Callback<Outcome>,
}

#[function_component(OfferRow)]
fn offer_row(props: &OfferProps) -> Html {
    let confirming = use_state(|| false);
    let busy = use_state(|| false);
    let booked = props.offer.booked;

    let request = ExamActionRequest {
        sm_tlst_objid: props.exam.sm_tlst_objid.clone(),
        year: props.exam.year.clone(),
        period: props.exam.period.clone(),
        offer_no: props.offer.offer_no.clone(),
        sc_objid: props.exam.sc_objid.clone(),
        ce_objid: props.offer.ce_objid.clone(),
    };
    let act = {
        let confirming = confirming.clone();
        let busy = busy.clone();
        let on_result = props.on_result.clone();
        let exam = props.exam.clone();
        let offer = props.offer.clone();
        Callback::from(move |_: MouseEvent| {
            if !*confirming {
                confirming.set(true);
                return;
            }
            confirming.set(false);
            busy.set(true);
            let path = if booked {
                "/api/exams/cancel"
            } else {
                "/api/exams/book"
            };
            let request = request.clone();
            let busy = busy.clone();
            let on_result = on_result.clone();
            let exam = exam.clone();
            let offer = offer.clone();
            spawn_local(async move {
                let outcome = api::post::<ExamActionResult, _>(path, &request)
                    .await
                    .map(|result| Done {
                        receipt: receipt::build(&exam, &offer, !booked, &result.message),
                        message: result.message,
                    })
                    .map_err(|failure| failure.message);
                busy.set(false);
                on_result.emit(outcome);
            });
        })
    };
    let abort = {
        let confirming = confirming.clone();
        Callback::from(move |_: MouseEvent| confirming.set(false))
    };

    let offer = &props.offer;
    let details: Vec<String> = [
        offer.date.clone(),
        offer.time.clone(),
        offer.room.clone(),
        offer.mode.clone(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect();
    let deadline = if booked {
        (!offer.deregistration_until.is_empty())
            .then(|| format!("Abmeldung bis {}", offer.deregistration_until))
    } else {
        (!offer.registration_until.is_empty())
            .then(|| format!("Anmeldung bis {}", offer.registration_until))
    };
    let label = match (booked, *confirming) {
        (false, false) => "Anmelden",
        (false, true) => "Verbindlich anmelden",
        (true, false) => "Abmelden",
        (true, true) => "Verbindlich abmelden",
    };
    let class = if booked && !*confirming {
        "button ghost compact"
    } else {
        "button primary compact"
    };

    html! {
        <div class="exam-offer">
            <div>
                <strong>{if details.is_empty() { "Termin offen".to_string() } else { details.join(" · ") }}</strong>
                {if booked { html! { <span class="exam-badge">{"Angemeldet"}</span> } } else { html! {} }}
                {if !offer.examiner.is_empty() { html! { <small>{format!("Prüfer: {}", offer.examiner)}</small> } } else { html! {} }}
                {deadline.map(|text| html! { <small>{text}</small> }).unwrap_or_default()}
            </div>
            <div class="exam-actions">
                <button class={class} onclick={act} disabled={*busy}>{label}</button>
                {if *confirming { html! { <button class="button ghost compact" onclick={abort}>{"Abbrechen"}</button> } } else { html! {} }}
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct ListProps {
    year: String,
    period: String,
    generation: u64,
}

#[function_component(ExamList)]
fn exam_list(props: &ListProps) -> Html {
    let reload = use_state(|| 0_u64);
    let feedback = use_state(|| None::<Outcome>);
    let exams = api::use_remote::<Vec<ExamEntry>>(
        format!("/api/exams?year={}&period={}", props.year, props.period),
        props.generation * 1000 + *reload,
    );
    let on_result = {
        let reload = reload.clone();
        let feedback = feedback.clone();
        Callback::from(move |outcome: Outcome| {
            let reload_needed = outcome.is_ok();
            feedback.set(Some(outcome));
            if reload_needed {
                reload.set(*reload + 1);
            }
        })
    };

    html! {
        <>
            {match &*feedback {
                Some(Ok(done)) => {
                    let receipt = done.receipt.clone();
                    let download = Callback::from(move |_: MouseEvent| receipt::download(&receipt));
                    html! {
                        <p class="exam-feedback ok" role="status">
                            <strong>{format!("{} erfolgreich. ", done.receipt.title)}</strong>
                            {done.message.as_str()}
                            {" "}
                            <button class="button ghost compact" onclick={download}>{"Beleg als Bild speichern"}</button>
                        </p>
                    }
                }
                Some(Err(message)) => html! { <p class="error" role="alert">{message}</p> },
                None => html! {},
            }}
            {match &*exams {
                Remote::Loading => html! { <p class="muted">{"Prüfungen werden geladen …"}</p> },
                Remote::Failed(error) => super::failure(error),
                Remote::Ready(items) if items.is_empty() => html! {
                    <p class="muted">{"In dieser Periode gibt es keine Prüfungen zur Anmeldung."}</p>
                },
                Remote::Ready(items) => html! {
                    <div class="exam-list">
                        {for items.iter().map(|exam| html! {
                            <div class="exam-entry">
                                <h3>{&exam.name}</h3>
                                {if exam.offers.is_empty() {
                                    html! { <p class="muted">{"Keine Termine angeboten."}</p> }
                                } else { html! {} }}
                                {for exam.offers.iter().map(|offer| html! {
                                    <OfferRow exam={exam.clone()} offer={offer.clone()} on_result={on_result.clone()} />
                                })}
                            </div>
                        })}
                    </div>
                },
            }}
        </>
    }
}

#[function_component(ExamRegistration)]
pub fn exam_registration(props: &RefreshProps) -> Html {
    let periods =
        api::use_remote::<Vec<ExamPeriod>>("/api/exams/periods".to_string(), props.generation);
    let selected = use_state(|| None::<(String, String)>);
    let on_select = {
        let selected = selected.clone();
        Callback::from(move |event: Event| {
            let value = event.target_unchecked_into::<HtmlSelectElement>().value();
            if let Some((year, id)) = value.split_once('/') {
                selected.set(Some((year.to_string(), id.to_string())));
            }
        })
    };

    html! {
        <section class="card exams-card">
            <div class="card-heading">
                <div>
                    <span class="eyebrow">{"PRÜFUNGEN"}</span>
                    <h2>{"Prüfungsanmeldung"}</h2>
                </div>
            </div>
            {match &*periods {
                Remote::Loading => html! { <p class="muted">{"Perioden werden geladen …"}</p> },
                Remote::Failed(error) => super::failure(error),
                Remote::Ready(items) => {
                    let current = (*selected).clone().or_else(|| {
                        items
                            .iter()
                            .find(|period| period.is_default)
                            .or_else(|| items.first())
                            .map(|period| (period.year.clone(), period.id.clone()))
                    });
                    match current {
                        None => html! { <p class="muted">{"Keine Prüfungsperioden verfügbar."}</p> },
                        Some((year, id)) => html! {
                            <>
                                <label class="sr-only" for="exam-period">{"Periode"}</label>
                                <select id="exam-period" onchange={on_select}>
                                    {for items.iter().map(|period| html! {
                                        <option value={format!("{}/{}", period.year, period.id)}
                                            selected={period.year == year && period.id == id}>
                                            {&period.title}
                                        </option>
                                    })}
                                </select>
                                <ExamList year={year} period={id} generation={props.generation} />
                            </>
                        },
                    }
                },
            }}
        </section>
    }
}
