use campusunbloat_shared::{StudyProgress, UserBasicInfo};
use yew::prelude::*;

use crate::api::{self, Remote};

#[derive(Properties, PartialEq)]
pub struct SummaryProps {
    pub user: UserBasicInfo,
    pub generation: u64,
}

fn number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string().replace('.', ",")
    }
}

#[function_component(Summary)]
pub fn summary(props: &SummaryProps) -> Html {
    let progress = api::use_remote::<StudyProgress>("/api/progress".to_string(), props.generation);
    let metrics = match &*progress {
        Remote::Loading => html! { <p class="muted">{"Studienfortschritt wird geladen …"}</p> },
        Remote::Failed(error) => super::failure(error),
        Remote::Ready(p) => html! {
            <div class="metrics">
                <div>
                    <span>{"Fachsemester"}</span>
                    <strong>{format!("{} / {}", p.semester, p.total_semesters)}</strong>
                </div>
                <div>
                    <span>{"ECTS: erreicht / möglich / Abschluss"}</span>
                    <strong>{format!("{} / {} / {}", number(p.ects), number(p.possible_ects), number(p.degree_ects))}</strong>
                </div>
            </div>
        },
    };

    html! {
        <section class="card summary-card">
            <div class="card-heading">
                <div>
                    <span class="eyebrow">{"PROFIL"}</span>
                    <h2>{"Deine Infos"}</h2>
                </div>
                <span class="card-symbol" aria-hidden="true">{"01"}</span>
            </div>
            <p class="profile-name">{format!("{} {}", props.user.first_name, props.user.last_name).trim().to_string()}</p>
            <p class="muted">{&props.user.user}</p>
            {if !props.user.seminar_name.is_empty() {
                html! { <p class="profile-program">{&props.user.seminar_name}</p> }
            } else {
                html! { <p class="muted">{"Studiengang nicht verfügbar."}</p> }
            }}
            {if !props.user.seminar_group.is_empty() {
                html! { <span class="pill">{&props.user.seminar_group}</span> }
            } else {
                html! {}
            }}
            {metrics}
        </section>
    }
}
