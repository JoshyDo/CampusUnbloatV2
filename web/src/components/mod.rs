pub mod calendar;
pub mod documents;
pub mod exams;
pub mod grades;
pub mod mensa;
pub mod opt_modules;
pub mod receipt;
pub mod reminders;
pub mod summary;

use yew::prelude::*;

use crate::api::ApiFailure;

#[derive(Properties, PartialEq)]
pub struct RefreshProps {
    pub generation: u64,
}

pub fn failure(error: &ApiFailure) -> Html {
    html! { <p class="error" role="alert">{&error.message}</p> }
}
