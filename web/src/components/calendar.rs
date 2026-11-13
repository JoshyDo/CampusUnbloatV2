use campusunbloat_shared::StundenplanItem;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Timelike, Utc};
use yew::prelude::*;

use crate::api::{self, Remote};

use super::RefreshProps;

const PIXELS_PER_HOUR: u32 = 60;

#[derive(Clone)]
struct PlacedEvent {
    item: StundenplanItem,
    start_minute: u32,
    end_minute: u32,
    lane: usize,
    lanes: usize,
}

fn local_time(timestamp: i64) -> Option<DateTime<Local>> {
    DateTime::<Utc>::from_timestamp_millis(timestamp).map(|value| value.with_timezone(&Local))
}

fn monday(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

fn module_name(event: &StundenplanItem) -> String {
    let title = event.title.trim();
    let parts: Vec<_> = title.split('-').collect();
    if parts.len() >= 3 && !parts[1].trim().is_empty() {
        parts[1].trim().to_string()
    } else if !title.is_empty() {
        title.to_string()
    } else {
        event.description.trim().to_string()
    }
}

fn valid_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

fn events_for_day(events: &[StundenplanItem], date: NaiveDate) -> Vec<PlacedEvent> {
    let mut placed: Vec<PlacedEvent> = events
        .iter()
        .filter_map(|event| {
            let start = local_time(event.start)?;
            let end = local_time(event.end)?;
            if start.date_naive() > date || end.date_naive() < date {
                return None;
            }
            let start_minute = if start.date_naive() < date {
                0
            } else {
                start.hour() * 60 + start.minute()
            };
            let end_minute = if end.date_naive() > date {
                24 * 60
            } else {
                end.hour() * 60 + end.minute()
            };
            if end_minute <= start_minute {
                return None;
            }
            Some(PlacedEvent {
                item: event.clone(),
                start_minute,
                end_minute,
                lane: 0,
                lanes: 1,
            })
        })
        .collect();
    placed.sort_by_key(|event| (event.start_minute, event.end_minute));

    let mut lane_ends: Vec<u32> = Vec::new();
    let mut group_start = 0;
    let mut group_end = 0;
    for index in 0..placed.len() {
        if index > group_start && placed[index].start_minute >= group_end {
            let lanes = lane_ends.len();
            for event in &mut placed[group_start..index] {
                event.lanes = lanes;
            }
            lane_ends.clear();
            group_start = index;
            group_end = 0;
        }
        let start = placed[index].start_minute;
        let end = placed[index].end_minute;
        let lane = lane_ends.iter().position(|lane_end| *lane_end <= start);
        placed[index].lane = match lane {
            Some(lane) => {
                lane_ends[lane] = end;
                lane
            }
            None => {
                lane_ends.push(end);
                lane_ends.len() - 1
            }
        };
        group_end = group_end.max(end);
    }
    let lanes = lane_ends.len();
    for event in &mut placed[group_start..] {
        event.lanes = lanes;
    }
    placed
}

fn weekday_name(index: usize) -> &'static str {
    ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"][index]
}

#[function_component(Calendar)]
pub fn calendar(props: &RefreshProps) -> Html {
    let events =
        api::use_remote::<Vec<StundenplanItem>>("/api/timetable".to_string(), props.generation);
    let week = use_state(|| monday(Local::now().date_naive()));
    let selected = use_state(|| None::<StundenplanItem>);
    let previous = {
        let week = week.clone();
        Callback::from(move |_| week.set(*week - Duration::days(7)))
    };
    let next = {
        let week = week.clone();
        Callback::from(move |_| week.set(*week + Duration::days(7)))
    };
    let today = {
        let week = week.clone();
        Callback::from(move |_| week.set(monday(Local::now().date_naive())))
    };
    let expanded = use_state(|| false);
    let toggle = {
        let expanded = expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };
    let close = {
        let selected = selected.clone();
        Callback::from(move |_| selected.set(None))
    };

    html! {
        <section class={classes!("card", "calendar-card", (!*expanded).then_some("is-half"))}>
            <div class="card-heading">
                <div>
                    <span class="eyebrow">{"STUNDENPLAN"}</span>
                    <h2>{"Kalender"}</h2>
                </div>
                <button class="button ghost compact" onclick={toggle}>
                    {if *expanded { "Verkleinern" } else { "Volle Größe" }}
                </button>
            </div>
            <div class="calendar-controls">
                <strong>{format!("{} – {}", week.format("%d.%m.%Y"), (*week + Duration::days(6)).format("%d.%m.%Y"))}</strong>
                <div class="control-group">
                    <button class="button secondary compact" onclick={today}>{"Heute"}</button>
                    <button class="icon-button" onclick={previous} aria-label="Vorherige Woche">{"←"}</button>
                    <button class="icon-button" onclick={next} aria-label="Nächste Woche">{"→"}</button>
                </div>
            </div>
            {match &*events {
                Remote::Loading => html! { <p class="muted">{"Stundenplan wird geladen …"}</p> },
                Remote::Failed(error) => super::failure(error),
                Remote::Ready(items) => {
                    let weekend_has_events = (5..7).any(|offset| {
                        !events_for_day(items, *week + Duration::days(offset)).is_empty()
                    });
                    let day_count = if weekend_has_events { 7 } else { 5 };
                    let days: Vec<_> = (0..day_count).map(|offset| {
                        let date = *week + Duration::days(offset as i64);
                        (date, events_for_day(items, date))
                    }).collect();
                    let first_hour = days.iter().flat_map(|(_, day)| day.iter())
                        .map(|event| event.start_minute / 60)
                        .min().unwrap_or(6).min(6);
                    let last_hour = days.iter().flat_map(|(_, day)| day.iter())
                        .map(|event| event.end_minute.div_ceil(60))
                        .max().unwrap_or(18).clamp(18, 24);
                    let height = (last_hour - first_hour) * PIXELS_PER_HOUR;
                    let column_min = if *expanded { 120 } else { 82 };
                    let grid_style = format!("grid-template-columns: 44px repeat({day_count}, minmax({column_min}px, 1fr));");
                    html! {
                        <>
                            {if days.iter().all(|(_, day)| day.is_empty()) {
                                html! { <p class="muted calendar-empty">{"Keine Termine in dieser Woche."}</p> }
                            } else { html! {} }}
                            <div class="calendar-scroll">
                                <div class="week-grid" style={grid_style}>
                                    <div class="calendar-day-heading"></div>
                                    {for days.iter().enumerate().map(|(index, (date, _))| html! {
                                        <div class={classes!("calendar-day-heading", (*date == Local::now().date_naive()).then_some("is-today"))}>
                                            <span>{weekday_name(index)}</span><strong>{date.format("%d.%m.").to_string()}</strong>
                                        </div>
                                    })}
                                    <div class="time-axis" style={format!("height:{height}px")}>
                                        {for (first_hour..=last_hour).map(|hour| html! {
                                            <span style={format!("top:{}px", (hour - first_hour) * PIXELS_PER_HOUR)}>{format!("{hour:02}:00")}</span>
                                        })}
                                    </div>
                                    {for days.iter().map(|(_, day)| html! {
                                        <div class="calendar-day" style={format!("height:{height}px")}>
                                            {for day.iter().map(|placed| {
                                                let item = placed.item.clone();
                                                let selected = selected.clone();
                                                let click = Callback::from(move |_| selected.set(Some(item.clone())));
                                                let top = placed.start_minute.saturating_sub(first_hour * 60);
                                                let height = (placed.end_minute - placed.start_minute).max(36);
                                                let width = 100.0 / placed.lanes as f64;
                                                let left = width * placed.lane as f64;
                                                let background = if valid_color(&placed.item.color) { placed.item.color.as_str() } else { "#315b8e" };
                                                let foreground = placed.item.font_color.as_deref().filter(|color| valid_color(color)).unwrap_or("#FFFFFF");
                                                let style = format!(
                                                    "top:{top}px;height:{height}px;left:calc({left:.4}% + 2px);width:calc({width:.4}% - 4px);background:{background};color:{foreground};"
                                                );
                                                let module = module_name(&placed.item);
                                                let room = if placed.item.room.trim().is_empty() { "offen" } else { placed.item.room.as_str() };
                                                let instructor = if placed.item.instructor.trim().is_empty() { "offen" } else { placed.item.instructor.as_str() };
                                                html! {
                                                    <button class="calendar-event" {style} onclick={click}
                                                        aria-label={format!("{module}, Raum {room}, Dozent {instructor}")}>
                                                        <strong>{module}</strong>
                                                        <span>{format!("Raum: {room}")}</span>
                                                        <span>{format!("Dozent: {instructor}")}</span>
                                                    </button>
                                                }
                                            })}
                                        </div>
                                    })}
                                </div>
                            </div>
                        </>
                    }
                },
            }}
            {if let Some(event) = &*selected {
                let module = module_name(event);
                let room = if event.room.trim().is_empty() { "Nicht angegeben" } else { event.room.as_str() };
                let instructor = if event.instructor.trim().is_empty() { "Nicht angegeben" } else { event.instructor.as_str() };
                html! {
                    <div class="modal-backdrop">
                        <section class="detail-modal" role="dialog" aria-modal="true" aria-labelledby="event-title">
                            <div class="card-heading">
                                <h2 id="event-title">{"Termininfo"}</h2>
                                <button class="icon-button" onclick={close} aria-label="Schließen" autofocus=true>{"×"}</button>
                            </div>
                            <dl class="event-details">
                                <dt>{"Modul"}</dt><dd>{module}</dd>
                                <dt>{"Raum"}</dt><dd>{room}</dd>
                                <dt>{"Dozent"}</dt><dd>{instructor}</dd>
                                {if !event.description.is_empty() { html! { <><dt>{"Beschreibung"}</dt><dd>{&event.description}</dd></> } } else { html! {} }}
                                {if !event.remarks.is_empty() { html! { <><dt>{"Info"}</dt><dd>{&event.remarks}</dd></> } } else { html! {} }}
                            </dl>
                        </section>
                    </div>
                }
            } else { html! {} }}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(title: &str) -> StundenplanItem {
        StundenplanItem {
            all_day: false,
            color: "#123456".into(),
            font_color: None,
            description: "Analysis".into(),
            editable: false,
            end: 0,
            instructor: String::new(),
            remarks: String::new(),
            room: String::new(),
            sinstructor: String::new(),
            sroom: String::new(),
            start: 0,
            title: title.into(),
        }
    }

    #[test]
    fn module_names_handle_exams_and_missing_short_codes() {
        assert_eq!(module_name(&event("5CS-MA2AN-00")), "MA2AN");
        assert_eq!(module_name(&event("P Analysis (K)")), "P Analysis (K)");
        assert_eq!(module_name(&event("")), "Analysis");
    }

    #[test]
    fn only_hex_colors_are_used_in_inline_styles() {
        assert!(valid_color("#FF0088"));
        assert!(!valid_color("#fff"));
        assert!(!valid_color("red;display:none"));
    }

    fn timed_event(start: i64, end: i64) -> StundenplanItem {
        StundenplanItem {
            start,
            end,
            ..event("5CS-MA2AN-00")
        }
    }

    #[test]
    fn overlapping_events_use_separate_lanes_but_adjacent_events_do_not() {
        let start = 1_789_974_000_000;
        let date = local_time(start).unwrap().date_naive();
        let hour = 3_600_000;
        let items = [
            timed_event(start, start + 2 * hour),
            timed_event(start + hour, start + 3 * hour),
            timed_event(start + 3 * hour, start + 4 * hour),
        ];
        let placed = events_for_day(&items, date);
        assert_eq!(placed.len(), 3);
        assert_eq!((placed[0].lane, placed[0].lanes), (0, 2));
        assert_eq!((placed[1].lane, placed[1].lanes), (1, 2));
        assert_eq!((placed[2].lane, placed[2].lanes), (0, 1));
    }

    #[test]
    fn events_ending_at_midnight_do_not_appear_on_the_next_day() {
        let date = NaiveDate::from_ymd_opt(2026, 9, 21).unwrap();
        let start = date
            .and_hms_opt(23, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .single()
            .unwrap()
            .timestamp_millis();
        let items = [timed_event(start, start + 3_600_000)];
        assert_eq!(events_for_day(&items, date).len(), 1);
        assert!(events_for_day(&items, date + Duration::days(1)).is_empty());
    }
}
