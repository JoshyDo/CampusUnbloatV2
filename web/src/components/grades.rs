use campusunbloat_shared::CampusDualGrade;
use chrono::NaiveDate;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api::{self, Remote};

use super::RefreshProps;

fn weighted_average(grades: &[CampusDualGrade]) -> Option<f64> {
    let (sum, credits) = grades
        .iter()
        .filter_map(|grade| {
            let value = grade.grade.replace(',', ".").parse::<f64>().ok()?;
            (value.is_finite() && (1.0..=5.0).contains(&value) && grade.credit_points > 0)
                .then_some((value * f64::from(grade.credit_points), grade.credit_points))
        })
        .fold((0.0, 0), |(sum, credits), (value, weight)| {
            (sum + value, credits + weight)
        });
    (credits > 0).then(|| sum / f64::from(credits))
}

fn readable_date(value: &str) -> Option<String> {
    NaiveDate::parse_from_str(value, "%Y%m%d")
        .ok()
        .map(|date| date.format("%d.%m.%Y").to_string())
}

/// Newest academic period first; modules without a period come last.
fn period_key(period: &str) -> (i32, i32) {
    let year = period
        .split(|c: char| !c.is_ascii_digit())
        .find(|part| part.len() == 4)
        .and_then(|part| part.parse().ok())
        .unwrap_or(0);
    (year, i32::from(period.contains("Sommer")))
}

fn group_by_period<'a>(
    grades: &'a [CampusDualGrade],
    query: &str,
) -> Vec<(String, Vec<&'a CampusDualGrade>)> {
    let mut groups: Vec<(String, Vec<&CampusDualGrade>)> = Vec::new();
    for grade in grades
        .iter()
        .filter(|grade| grade.name.to_lowercase().contains(query))
    {
        match groups
            .iter_mut()
            .find(|(period, _)| *period == grade.akad_period)
        {
            Some((_, modules)) => modules.push(grade),
            None => groups.push((grade.akad_period.clone(), vec![grade])),
        }
    }
    groups.sort_by_key(|(period, _)| std::cmp::Reverse(period_key(period)));
    groups
}

fn grade_entry(grade: &CampusDualGrade) -> Html {
    html! {
        <details class="grade-entry">
            <summary>
                <span>{&grade.name}</span>
                <strong>{if grade.grade.is_empty() { "–" } else { &grade.grade }}</strong>
            </summary>
            <div class="grade-details">
                <p>{format!("{} ECTS", grade.credit_points)}</p>
                {for grade.subgrades.iter().map(|subgrade| html! {
                    <div class="subgrade">
                        <span>
                            {&subgrade.name}
                            {readable_date(&subgrade.bekanntgabe).map(|date| html! {
                                <small>{format!("Bekanntgabe: {date}")}</small>
                            }).unwrap_or_default()}
                        </span>
                        <strong>{if subgrade.grade.is_empty() { "–" } else { &subgrade.grade }}</strong>
                    </div>
                })}
            </div>
        </details>
    }
}

#[function_component(Grades)]
pub fn grades(props: &RefreshProps) -> Html {
    let grades =
        api::use_remote::<Vec<CampusDualGrade>>("/api/grades".to_string(), props.generation);
    let filter = use_state(String::new);
    let on_filter = {
        let filter = filter.clone();
        Callback::from(move |event: InputEvent| {
            filter.set(event.target_unchecked_into::<HtmlInputElement>().value());
        })
    };

    html! {
        <section class="card grades-card">
            <div class="card-heading">
                <div>
                    <span class="eyebrow">{"STUDIENLEISTUNGEN"}</span>
                    <h2>{"Noten"}</h2>
                </div>
                <span class="card-symbol" aria-hidden="true">{"02"}</span>
            </div>
            {match &*grades {
                Remote::Loading => html! { <p class="muted">{"Noten werden geladen …"}</p> },
                Remote::Failed(error) => super::failure(error),
                Remote::Ready(items) => {
                    let average = weighted_average(items)
                        .map(|value| format!("{value:.2}").replace('.', ","))
                        .unwrap_or_else(|| "–".to_string());
                    let query = filter.to_lowercase();
                    html! {
                        <>
                            <div class="grade-overview">
                                <div><strong>{average}</strong><span>{"ECTS-gewichteter Schnitt"}</span></div>
                                <div><strong>{items.len()}</strong><span>{"Module"}</span></div>
                            </div>
                            <label class="sr-only" for="grade-filter">{"Noten nach Modul filtern"}</label>
                            <input id="grade-filter" class="search" type="search" placeholder="Modul suchen"
                                value={(*filter).clone()} oninput={on_filter} />
                            <div class="grade-list">
                                {for group_by_period(items, &query).into_iter().map(|(period, modules)| {
                                    let ects: i32 = modules.iter().map(|grade| grade.credit_points).sum();
                                    let owned: Vec<CampusDualGrade> = modules.iter().map(|grade| (*grade).clone()).collect();
                                    let avg = weighted_average(&owned)
                                        .map(|value| format!(" · Ø {value:.2}").replace('.', ","))
                                        .unwrap_or_default();
                                    html! {
                                        <section class="grade-group">
                                            <h3 class="grade-group-title">
                                                <span>{if period.is_empty() { "Ohne Zeitraum".to_string() } else { period }}</span>
                                                <small>{format!("{} Module · {ects} ECTS{avg}", modules.len())}</small>
                                            </h3>
                                            {for modules.into_iter().map(grade_entry)}
                                        </section>
                                    }
                                })}
                            </div>
                            {if items.is_empty() {
                                html! { <p class="muted">{"Es sind keine Noten vorhanden."}</p> }
                            } else if !items.iter().any(|grade| grade.name.to_lowercase().contains(&query)) {
                                html! { <p class="muted">{"Kein Modul entspricht der Suche."}</p> }
                            } else { html! {} }}
                        </>
                    }
                },
            }}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_are_grouped_newest_first() {
        let mut a = grade("1,0", 5);
        a.akad_period = "Akad. Jahr 2025/2026 Sommerperiode".into();
        let mut b = grade("2,0", 5);
        b.akad_period = "Akad. Jahr 2026/2027 Winterperiode".into();
        let mut c = grade("3,0", 5);
        c.akad_period = a.akad_period.clone();
        let none = grade("1,0", 5);
        let all = [a, none, b, c];
        let groups = group_by_period(&all, "");
        let sizes: Vec<_> = groups.iter().map(|(p, m)| (p.as_str(), m.len())).collect();
        assert_eq!(
            sizes,
            [
                ("Akad. Jahr 2026/2027 Winterperiode", 1),
                ("Akad. Jahr 2025/2026 Sommerperiode", 2),
                ("", 1)
            ]
        );
    }

    fn grade(value: &str, credits: i32) -> CampusDualGrade {
        CampusDualGrade {
            name: String::new(),
            grade: value.into(),
            total_passed: None,
            credit_points: credits,
            akad_period: String::new(),
            subgrades: Vec::new(),
        }
    }

    #[test]
    fn average_is_weighted_and_ignores_missing_or_invalid_grades() {
        let grades = [
            grade("1,0", 5),
            grade("2,0", 10),
            grade("", 5),
            grade("NaN", 5),
        ];
        assert!((weighted_average(&grades).unwrap() - 5.0 / 3.0).abs() < 0.0001);
        assert_eq!(weighted_average(&[grade("", 5)]), None);
        assert_eq!(weighted_average(&[grade("1,0", 0)]), None);
    }
}
