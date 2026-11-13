use std::hash::{Hash, Hasher};

use anyhow::{Context, Result};
use axum::{Extension, Json, extract::Query};
use campusunbloat_shared::{
    CampusDualGrade, CampusReminders, Documents, ExamActionRequest, ExamActionResult, ExamEntry,
    ExamPeriod, OptModules, StudyProgress, StundenplanItem,
};
use fnv::FnvHasher;

use crate::{
    auth::SessionContext,
    campus_backend::odata::{
        DocumentRequest, ExamRejected, client_from_cookies, courses_to_grades, exam_action,
        fetch_courses, fetch_document_pdf, fetch_documents, fetch_exam_periods, fetch_exams,
        fetch_fachsemester, fetch_opt_modules, fetch_reminders, fetch_timetable,
    },
    color_stuff::hex_to_luminance,
    error::AppError,
};

#[derive(serde::Deserialize)]
pub struct ExamQuery {
    year: String,
    period: String,
}

pub async fn documents(
    Extension(session): Extension<SessionContext>,
) -> Result<Json<Documents>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    Ok(Json(fetch_documents(&client).await?))
}

pub async fn document_pdf(
    Extension(session): Extension<SessionContext>,
    Query(request): Query<DocumentRequest>,
) -> Result<axum::response::Response, AppError> {
    use axum::{http::header, response::IntoResponse};
    let client = client_from_cookies(&session.auth.cookie)?;
    let pdf = fetch_document_pdf(&client, &request).await?;
    let name: String = request
        .form
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}-{}.pdf\"", request.created),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        pdf,
    )
        .into_response())
}

pub async fn opt_modules(
    Extension(session): Extension<SessionContext>,
) -> Result<Json<OptModules>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    Ok(Json(fetch_opt_modules(&client).await?))
}

pub async fn exam_periods(
    Extension(session): Extension<SessionContext>,
) -> Result<Json<Vec<ExamPeriod>>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    Ok(Json(fetch_exam_periods(&client).await?))
}

pub async fn exams(
    Extension(session): Extension<SessionContext>,
    Query(query): Query<ExamQuery>,
) -> Result<Json<Vec<ExamEntry>>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    Ok(Json(
        fetch_exams(&client, &query.year, &query.period).await?,
    ))
}

pub async fn open_exams(
    Extension(session): Extension<SessionContext>,
) -> Result<Json<Vec<ExamEntry>>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    let mut open = Vec::new();
    for period in fetch_exam_periods(&client).await?.into_iter().take(3) {
        for mut exam in fetch_exams(&client, &period.year, &period.id).await? {
            exam.offers.retain(|offer| !offer.booked);
            if !exam.offers.is_empty() {
                open.push(exam);
            }
        }
    }
    Ok(Json(open))
}

fn exam_error(error: anyhow::Error) -> AppError {
    match error.downcast_ref::<ExamRejected>() {
        Some(rejected) => AppError::new(
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            rejected.0.clone(),
        ),
        None => error.into(),
    }
}

pub async fn book_exam(
    Extension(session): Extension<SessionContext>,
    Json(request): Json<ExamActionRequest>,
) -> Result<Json<ExamActionResult>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    let message = exam_action(&client, true, &request)
        .await
        .map_err(exam_error)?;
    Ok(Json(ExamActionResult { message }))
}

pub async fn cancel_exam(
    Extension(session): Extension<SessionContext>,
    Json(request): Json<ExamActionRequest>,
) -> Result<Json<ExamActionResult>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    let message = exam_action(&client, false, &request)
        .await
        .map_err(exam_error)?;
    Ok(Json(ExamActionResult { message }))
}

pub async fn grades(
    Extension(session): Extension<SessionContext>,
) -> Result<Json<Vec<CampusDualGrade>>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    Ok(Json(courses_to_grades(fetch_courses(&client).await?)?))
}

pub async fn progress(
    Extension(session): Extension<SessionContext>,
) -> Result<Json<StudyProgress>, AppError> {
    // Campus Dual exposes no degree totals; these are the regular Bachelor values.
    const DEGREE_ECTS: f64 = 180.0;
    const TOTAL_SEMESTERS: u32 = 6;

    let client = client_from_cookies(&session.auth.cookie)?;
    let courses = fetch_courses(&client).await?;
    let mut ects = 0.0;
    let mut possible_ects = 0.0;
    for course in &courses {
        let credits = course.credits()?;
        possible_ects += credits;
        if course.passed() {
            ects += credits;
        }
    }
    Ok(Json(StudyProgress {
        semester: fetch_fachsemester(&client).await?,
        total_semesters: TOTAL_SEMESTERS,
        ects,
        possible_ects,
        degree_ects: DEGREE_ECTS,
    }))
}

pub async fn ects(Extension(session): Extension<SessionContext>) -> Result<Json<f64>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    let courses = fetch_courses(&client).await?;
    let total = courses
        .iter()
        .filter(|course| course.passed())
        .try_fold(0.0, |sum, course| {
            Ok::<_, anyhow::Error>(sum + course.credits()?)
        })?;
    Ok(Json(total))
}

pub async fn fachsemester(
    Extension(session): Extension<SessionContext>,
) -> Result<Json<u32>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    Ok(Json(fetch_fachsemester(&client).await?))
}

pub async fn timetable(
    Extension(session): Extension<SessionContext>,
) -> Result<Json<Vec<StundenplanItem>>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    let mut events = fetch_timetable(&client).await?;
    format_timetable(&mut events)?;
    Ok(Json(events))
}

fn format_timetable(events: &mut [StundenplanItem]) -> Result<()> {
    for event in events {
        event.start = event
            .start
            .checked_mul(1000)
            .context("Invalid event start")?;
        event.end = event.end.checked_mul(1000).context("Invalid event end")?;
        event.color = if event.color == "darkred" {
            "#D41610".to_string()
        } else {
            let mut hasher = FnvHasher::default();
            format!("0{}0", event.title).hash(&mut hasher);
            let hash = hasher.finish();
            format!(
                "#{:02X}{:02X}{:02X}",
                hash & 255,
                (hash >> 8) & 255,
                (hash >> 16) & 255
            )
        };
        event.font_color = Some(
            if hex_to_luminance(&event.color) < 128.0 {
                "#FFFFFF"
            } else {
                "#000000"
            }
            .to_string(),
        );
    }
    Ok(())
}

pub async fn reminders(
    Extension(session): Extension<SessionContext>,
) -> Result<Json<CampusReminders>, AppError> {
    let client = client_from_cookies(&session.auth.cookie)?;
    Ok(Json(fetch_reminders(&client).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exams_stay_red_and_times_are_milliseconds() {
        let mut events = [StundenplanItem {
            all_day: false,
            color: "darkred".into(),
            font_color: None,
            description: String::new(),
            editable: false,
            end: 2,
            instructor: String::new(),
            remarks: String::new(),
            room: String::new(),
            sinstructor: String::new(),
            sroom: String::new(),
            start: 1,
            title: "5CS-MA2AN-00".into(),
        }];
        format_timetable(&mut events).unwrap();
        assert_eq!(events[0].color, "#D41610");
        assert_eq!(events[0].start, 1000);
        assert_eq!(events[0].end, 2000);
    }
}
