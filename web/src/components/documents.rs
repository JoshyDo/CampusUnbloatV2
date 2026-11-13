use campusunbloat_shared::Documents as DocumentList;
use web_sys::HtmlSelectElement;
use yew::prelude::*;

use crate::api::{self, Remote};

use super::RefreshProps;

#[function_component(Documents)]
pub fn documents(props: &RefreshProps) -> Html {
    let data = api::use_remote::<DocumentList>("/api/documents".to_string(), props.generation);
    let selected = use_state(|| None::<usize>);

    html! {
        <section class="card">
            <div class="card-heading">
                <div>
                    <span class="eyebrow">{"DOKUMENTE"}</span>
                    <h2>{"Studentendokumente"}</h2>
                </div>
            </div>
            {match &*data {
                Remote::Loading => html! { <p class="muted">{"Dokumente werden geladen …"}</p> },
                Remote::Failed(error) => super::failure(error),
                Remote::Ready(list) if list.semesters.is_empty() || list.forms.is_empty() => {
                    html! { <p class="muted">{"Keine Dokumente verfügbar."}</p> }
                }
                Remote::Ready(list) => {
                    let index = selected.unwrap_or(0).min(list.semesters.len() - 1);
                    let semester = &list.semesters[index];
                    let on_change = {
                        let selected = selected.clone();
                        Callback::from(move |event: Event| {
                            let value = event.target_unchecked_into::<HtmlSelectElement>().value();
                            selected.set(value.parse().ok());
                        })
                    };
                    html! {
                        <>
                            <label class="sr-only" for="doc-semester">{"Semester"}</label>
                            <select id="doc-semester" onchange={on_change}>
                                {for list.semesters.iter().enumerate().map(|(i, s)| html! {
                                    <option value={i.to_string()} selected={i == index}>{&s.title}</option>
                                })}
                            </select>
                            <div class="exam-list">
                                {for list.forms.iter().map(|form| {
                                    let href = format!(
                                        "/api/documents/pdf?form={}&student={}&created={}&year={}&period={}",
                                        js_sys::encode_uri_component(&form.name),
                                        js_sys::encode_uri_component(&form.student_id),
                                        form.created,
                                        semester.year,
                                        semester.id
                                    );
                                    html! {
                                        <div class="exam-offer">
                                            <strong>{&form.title}</strong>
                                            <a class="button ghost compact" href={href} download="">{"PDF laden"}</a>
                                        </div>
                                    }
                                })}
                            </div>
                        </>
                    }
                }
            }}
        </section>
    }
}
