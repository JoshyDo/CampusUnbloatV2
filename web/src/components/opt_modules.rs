use campusunbloat_shared::OptModules;
use yew::prelude::*;

use crate::api::{self, Remote};

use super::RefreshProps;

#[function_component(OptModulesCard)]
pub fn opt_modules_card(props: &RefreshProps) -> Html {
    let data = api::use_remote::<OptModules>("/api/opt-modules".to_string(), props.generation);

    match &*data {
        Remote::Loading => html! {
            <section class="card"><p class="muted">{"Wahlpflichtmodule werden geladen …"}</p></section>
        },
        Remote::Failed(error) => html! { <section class="card">{super::failure(error)}</section> },
        Remote::Ready(data) => {
            let open: Vec<_> = data.periods.iter().filter(|period| period.open).collect();
            let available: usize = open
                .iter()
                .flat_map(|period| &period.modules)
                .filter(|module| module.can_register)
                .count();
            html! {
                <details class="card collapsible" open={!open.is_empty()}>
                    <summary>
                        <span class="eyebrow">{"WAHLPFLICHTMODULE"}</span>
                        <h2>{"Anmeldung Wahlpflichtmodule"}</h2>
                        {if available > 0 {
                            html! { <span class="count">{available}</span> }
                        } else if open.is_empty() {
                            html! { <small class="muted">{"Aktuell keine Anmeldung möglich"}</small> }
                        } else {
                            html! { <small class="muted">{"Anmeldung offen, keine Module"}</small> }
                        }}
                    </summary>
                    {if open.is_empty() {
                        html! { <p class="muted">{"Es ist keine Anmeldephase geöffnet."}</p> }
                    } else { html! {} }}
                    {for open.iter().map(|period| html! {
                        <div class="opt-period">
                            <h3>{&period.title}</h3>
                            {if period.modules.is_empty() {
                                html! { <p class="muted">{"Für diese Periode sind noch keine Module freigegeben."}</p> }
                            } else { html! {} }}
                            {for period.modules.iter().map(|module| html! {
                                <div class="exam-offer">
                                    <div>
                                        <strong>{&module.title}</strong>
                                        <small>{&module.subtitle}</small>
                                        <small>{format!("{} · Fachsemester {} · freie Plätze {}", module.group, module.semester, module.free_capacity)}</small>
                                        {if !module.status.is_empty() { html! { <span class="exam-badge">{&module.status}</span> } } else { html! {} }}
                                    </div>
                                    <div class="exam-actions">
                                        {if module.can_register || module.can_deregister {
                                            html! { <a class="button ghost compact" href="https://fep.campus-dual.de/" target="_blank" rel="noopener noreferrer">{"In Campus Dual anmelden"}</a> }
                                        } else { html! {} }}
                                    </div>
                                </div>
                            })}
                        </div>
                    })}
                </details>
            }
        }
    }
}
