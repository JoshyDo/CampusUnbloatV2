use campusunbloat_shared::{LoginRequest, UserBasicInfo};
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;

mod api;
mod components;

use components::{
    calendar::Calendar, documents::Documents, exams::ExamRegistration, grades::Grades,
    mensa::Mensa, opt_modules::OptModulesCard, reminders::Reminders, summary::Summary,
};

#[derive(Clone, PartialEq)]
enum SessionView {
    Loading,
    LoggedOut,
    LoggedIn(UserBasicInfo),
    Failed(api::ApiFailure),
}

#[function_component(App)]
fn app() -> Html {
    let session = use_state(|| SessionView::Loading);
    let attempt = use_state(|| 0_u64);
    {
        let session = session.clone();
        use_effect_with(*attempt, move |_| {
            session.set(SessionView::Loading);
            spawn_local(async move {
                match api::get::<UserBasicInfo>("/api/session").await {
                    Ok(user) => session.set(SessionView::LoggedIn(user)),
                    Err(error) if error.status == Some(401) => session.set(SessionView::LoggedOut),
                    Err(error) => session.set(SessionView::Failed(error)),
                }
            });
            || ()
        });
    }

    let on_login = {
        let session = session.clone();
        Callback::from(move |user| session.set(SessionView::LoggedIn(user)))
    };
    let on_logout = {
        let session = session.clone();
        Callback::from(move |()| session.set(SessionView::LoggedOut))
    };
    let retry = {
        let attempt = attempt.clone();
        Callback::from(move |_| attempt.set(*attempt + 1))
    };

    html! {
        <>
            <header class="topbar">
                <a class="brand" href="/">
                    <span class="brand-mark" aria-hidden="true">{"CU"}</span>
                    <span>{"CampusUnbloat"}<small>{"V2"}</small></span>
                </a>
                <span class="topbar-note">{"Dein Studium auf einen Blick"}</span>
            </header>
            <main class="page">
                {match &*session {
                    SessionView::Loading => html! { <p class="loading">{"Sitzung wird geprüft …"}</p> },
                    SessionView::LoggedOut => html! { <Login {on_login} /> },
                    SessionView::LoggedIn(user) => html! {
                        <Dashboard user={user.clone()} {on_logout} />
                    },
                    SessionView::Failed(error) => html! {
                        <section class="login-panel">
                            <h1>{"Verbindung fehlgeschlagen"}</h1>
                            {components::failure(error)}
                            <button class="button primary" onclick={retry}>{"Erneut versuchen"}</button>
                        </section>
                    },
                }}
            </main>
        </>
    }
}

#[derive(Properties, PartialEq)]
struct LoginProps {
    on_login: Callback<UserBasicInfo>,
}

#[function_component(Login)]
fn login(props: &LoginProps) -> Html {
    let username = use_state(String::new);
    let password = use_state(String::new);
    let remember = use_state(|| true);
    let loading = use_state(|| false);
    let error = use_state(|| None::<api::ApiFailure>);

    let on_username = {
        let username = username.clone();
        Callback::from(move |event: InputEvent| {
            username.set(event.target_unchecked_into::<HtmlInputElement>().value());
        })
    };
    let on_password = {
        let password = password.clone();
        Callback::from(move |event: InputEvent| {
            password.set(event.target_unchecked_into::<HtmlInputElement>().value());
        })
    };
    let on_remember = {
        let remember = remember.clone();
        Callback::from(move |event: Event| {
            remember.set(event.target_unchecked_into::<HtmlInputElement>().checked());
        })
    };
    let onsubmit = {
        let username = username.clone();
        let password = password.clone();
        let remember = remember.clone();
        let loading = loading.clone();
        let error = error.clone();
        let on_login = props.on_login.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            if *loading {
                return;
            }
            let request = LoginRequest {
                username: (*username).clone(),
                password: (*password).clone(),
                remember: *remember,
            };
            password.set(String::new());
            error.set(None);
            loading.set(true);
            let loading = loading.clone();
            let error = error.clone();
            let on_login = on_login.clone();
            spawn_local(async move {
                match api::post::<UserBasicInfo, _>("/api/login", &request).await {
                    Ok(user) => on_login.emit(user),
                    Err(failure) => error.set(Some(failure)),
                }
                loading.set(false);
            });
        })
    };

    html! {
        <section class="login-panel">
            <span class="eyebrow">{"CAMPUS DUAL"}</span>
            <h1>{"Willkommen zurück."}</h1>
            <p class="muted">{"Melde dich mit deinem Campus-Dual-Konto an."}</p>
            <form {onsubmit} class="login-form">
                <label for="username">{"Benutzername"}</label>
                <input id="username" name="username" autocomplete="username" required=true
                    value={(*username).clone()} oninput={on_username} />
                <label for="password">{"Passwort"}</label>
                <input id="password" name="password" type="password" autocomplete="current-password"
                    required=true value={(*password).clone()} oninput={on_password} />
                <label class="remember-row">
                    <input type="checkbox" checked={*remember} onchange={on_remember} />
                    <span>{"Angemeldet bleiben (30 Tage, verschlüsselt auf diesem Gerät)"}</span>
                </label>
                {if let Some(error) = &*error { components::failure(error) } else { html! {} }}
                                <button class="button primary" type="submit" disabled={*loading}>
                    {if *loading { "Anmeldung läuft …" } else { "Anmelden" }}
                </button>
            </form>
            <p class="privacy-note">
                {"Die Anmeldung wird nur an Campus Dual weitergegeben. Ohne \"Angemeldet bleiben\" bleibt die Sitzung nur im Arbeitsspeicher, sonst liegen die Zugangsdaten verschlüsselt nur in einem Cookie auf deinem Gerät, nicht auf dem Server. Abmelden löscht es."}
            </p>
        </section>
    }
}

#[derive(Properties, PartialEq)]
struct DashboardProps {
    user: UserBasicInfo,
    on_logout: Callback<()>,
}

#[function_component(Dashboard)]
fn dashboard(props: &DashboardProps) -> Html {
    let generation = use_state(|| 0_u64);
    let logout_error = use_state(|| None::<api::ApiFailure>);
    let logging_out = use_state(|| false);
    let refresh = {
        let generation = generation.clone();
        Callback::from(move |_| generation.set(*generation + 1))
    };
    let logout = {
        let on_logout = props.on_logout.clone();
        let logout_error = logout_error.clone();
        let logging_out = logging_out.clone();
        Callback::from(move |_| {
            if *logging_out {
                return;
            }
            logging_out.set(true);
            logout_error.set(None);
            let on_logout = on_logout.clone();
            let logout_error = logout_error.clone();
            let logging_out = logging_out.clone();
            spawn_local(async move {
                match api::logout().await {
                    Ok(()) => on_logout.emit(()),
                    Err(error) => logout_error.set(Some(error)),
                }
                logging_out.set(false);
            });
        })
    };
    let greeting = if props.user.first_name.is_empty() {
        &props.user.user
    } else {
        &props.user.first_name
    };

    html! {
        <>
            <div class="dashboard-heading">
                <div>
                    <span class="eyebrow">{"DEIN DASHBOARD"}</span>
                    <h1>{format!("Hallo, {greeting}.")}</h1>
                    <p class="muted">{"Alles Wichtige für dein Studium."}</p>
                </div>
                <div class="heading-actions">
                    <Reminders generation={*generation} />
                    <button class="button secondary" onclick={refresh}>{"Aktualisieren"}</button>
                    <button class="button ghost" onclick={logout} disabled={*logging_out}>{"Abmelden"}</button>
                </div>
            </div>
            {if let Some(error) = &*logout_error { components::failure(error) } else { html! {} }}
            <div class="dashboard-grid">
                <Summary user={props.user.clone()} generation={*generation} />
                <Grades generation={*generation} />
                <Calendar generation={*generation} />
                <Mensa generation={*generation} />
                <ExamRegistration generation={*generation} />
                <OptModulesCard generation={*generation} />
                <Documents generation={*generation} />
                <section class="card info-card">
                    <span class="eyebrow">{"STUDIENABLAUF"}</span>
                    <h2>{"Blockplan"}</h2>
                    <p class="muted">{"Für Theorie- und Praxisphasen ist in Campus Dual V2 noch keine verlässliche Datenquelle bekannt."}</p>
                </section>
            </div>
        </>
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
