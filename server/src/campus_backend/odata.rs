use std::collections::HashMap;

use anyhow::{Context, Result, anyhow, ensure};
use lazy_static::lazy_static;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, de::DeserializeOwned};
use std::time::Duration;

use crate::{
    campus_backend::login::cookie_store_from_json,
    constants::CD_CERT_PEM,
    types::{
        CampusDualGrade, CampusDualSubGrade, CampusReminders, DocumentForm, DocumentSemester,
        Documents, ExamActionRequest, ExamEntry, ExamOffer, ExamPeriod, LatestReminder, OptModule,
        OptModules, OptPeriod, StundenplanItem, UpcomingReminder, UserBasicInfo,
    },
};

const ODATA_BASE: &str = "https://fep.campus-dual.de/sap/opu/odata/sap";

pub fn client_from_cookies(j_cookies: &str) -> Result<Client> {
    Ok(reqwest::Client::builder()
        .add_root_certificate(CD_CERT_PEM.get().unwrap().clone())
        .cookie_provider(cookie_store_from_json(j_cookies)?)
        .timeout(Duration::from_secs(30))
        .build()?)
}

#[derive(Deserialize)]
struct ODataList<T> {
    d: ODataResults<T>,
}

#[derive(Deserialize)]
struct ODataResults<T> {
    results: Vec<T>,
}

#[derive(Deserialize)]
struct ODataEntity<T> {
    d: T,
}

async fn odata_get_text(client: &Client, path: &str) -> Result<String> {
    odata_get_text_at(client, ODATA_BASE, path).await
}

async fn odata_get_text_at(client: &Client, base: &str, path: &str) -> Result<String> {
    let url = format!("{base}/{path}");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .header("DataServiceVersion", "2.0")
        .header("MaxDataServiceVersion", "2.0")
        .send()
        .await?
        .error_for_status()?;

    // An expired session is answered with the IdP login page instead of JSON.
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();

    let is_json = content_type.contains("json");
    if !is_json {
        log::warn!(
            "OData response for {} is not JSON (content-type: {})",
            url,
            content_type
        );
        anyhow::bail!("OData response is not JSON (session expired?)");
    }
    Ok(resp.text().await?)
}

async fn odata_list<T: DeserializeOwned>(client: &Client, path: &str) -> Result<Vec<T>> {
    let text = odata_get_text(client, path).await?;
    Ok(serde_json::from_str::<ODataList<T>>(&text)?.d.results)
}

/// True if the session cookies are still accepted by the SAP backend.
pub async fn session_alive(client: &Client) -> Result<bool> {
    let url = format!(
        "{ODATA_BASE}/ZCM_EM_STUDENT_TIMETABLE_SRV/EventListSet?sap-client=100&$top=1&$select=Objid"
    );
    let response = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await?;
    if response.url().host_str() != Some("fep.campus-dual.de")
        || matches!(
            response.status(),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        )
    {
        return Ok(false);
    }
    let response = response.error_for_status()?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if content_type.contains("json") {
        Ok(true)
    } else if content_type.contains("html") {
        Ok(false)
    } else {
        Err(anyhow!("Unexpected Campus Dual session response type"))
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HeaderDetails {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Faculty")]
    faculty: String,
    #[serde(rename = "ProgramOfStudy")]
    program_of_study: String,
    #[serde(rename = "StudentNumber")]
    student_number: String,
    #[serde(rename = "TitleText")]
    title_text: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CohortRaw {
    #[serde(rename = "Cohorts")]
    cohorts: String,
}

/// Seminar group (e.g. "5CS25-2") from the most common timetable cohort,
/// formatted like "Informatik 2025 Gr. 2 (5CS25-2)".
async fn fetch_seminar_group(client: &Client) -> Option<String> {
    let rows: Vec<CohortRaw> = odata_list(
        client,
        "ZCM_EM_STUDENT_TIMETABLE_SRV/EventListSet?sap-client=100&$skip=0&$top=200&$select=Cohorts",
    )
    .await
    .map_err(|e| log::warn!("Could not fetch cohorts: {e:?}"))
    .ok()?;

    let mut counts: HashMap<String, usize> = HashMap::new();
    for r in rows {
        let c = r.cohorts.trim();
        if c.is_empty() {
            continue;
        }
        let group = match (c.rfind('('), c.rfind(')')) {
            (Some(start), Some(end)) if start < end => c[start + 1..end].trim(),
            _ => c,
        };
        *counts.entry(group.to_string()).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(group, _)| group)
}

async fn fetch_header_details(client: &Client) -> Result<HeaderDetails> {
    let text = odata_get_text(
        client,
        "PIQ_STUDENT_PERSONAL_DETAILS_SRV/HeaderDetailsSet(StudentObjid='')?sap-client=100",
    )
    .await?;
    Ok(serde_json::from_str::<ODataEntity<HeaderDetails>>(&text)?.d)
}

pub async fn fetch_user_info(client: &Client, username: &str) -> Result<UserBasicInfo> {
    match fetch_header_details(client).await {
        Ok(details) => {
            // Name is "<Title> First Middle Last"; the last token is the surname
            let mut full_name = details.name.trim();
            let title = details.title_text.trim();
            if !title.is_empty()
                && let Some(rest) = full_name.strip_prefix(title)
            {
                full_name = rest.trim_start();
            }
            let (first_name, last_name) = match full_name.rsplit_once(' ') {
                Some((first, last)) => (first.to_string(), last.to_string()),
                None => (String::new(), full_name.to_string()),
            };

            let program = details
                .program_of_study
                .split(" (")
                .next()
                .unwrap_or_default()
                .trim();
            let program = if program.is_empty() {
                details.faculty.trim()
            } else {
                program
            };
            let seminar_name = program.strip_prefix("SG ").unwrap_or(program).to_string();

            Ok(UserBasicInfo {
                first_name,
                last_name,
                seminar_group: fetch_seminar_group(client).await.unwrap_or_default(),
                seminar_name,
                user: if details.student_number.is_empty() {
                    username.to_string()
                } else {
                    details.student_number
                },
            })
        }
        Err(e) => {
            log::warn!(
                "Could not fetch personal details via OData ({e:?}), falling back to username"
            );
            Ok(UserBasicInfo {
                first_name: String::new(),
                last_name: username.to_string(),
                seminar_group: String::new(),
                seminar_name: String::new(),
                user: username.to_string(),
            })
        }
    }
}

pub async fn fetch_fachsemester(client: &Client) -> Result<u32> {
    lazy_static! {
        static ref RE_FS: Regex = Regex::new(r"Fachsemester\s+(\d+)").unwrap();
    }
    let details = fetch_header_details(client).await?;
    RE_FS
        .captures(&details.program_of_study)
        .and_then(|c| c[1].parse().ok())
        .context("Fachsemester is not present in the Campus Dual response")
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct EventRaw {
    #[serde(rename = "EventTitle")]
    title: String,
    #[serde(rename = "EventShort")]
    short: String,
    #[serde(rename = "EventDescription")]
    description: String,
    #[serde(rename = "LecturerName")]
    lecturer: String,
    #[serde(rename = "Room")]
    room: String,
    #[serde(rename = "Remarks")]
    remarks: String,
    #[serde(rename = "ExamCategory")]
    exam_category: String,
    #[serde(rename = "ExamShort")]
    exam_short: String,
    #[serde(rename = "ExaminerName")]
    examiner: String,
    #[serde(rename = "AppointmentType")]
    appointment_type: String,
    #[serde(rename = "Start")]
    start: String,
    #[serde(rename = "End")]
    end: String,
}

/// "/Date(1782712800000)/" -> 1782712800 (seconds)
fn odata_date_to_secs(input: &str) -> Result<i64> {
    lazy_static! {
        static ref RE_DATE: Regex = Regex::new(r"^/Date\((-?\d+)(?:[+-]\d{4})?\)/$").unwrap();
    }
    RE_DATE
        .captures(input)
        .and_then(|c| c[1].parse::<i64>().ok())
        .map(|ms| ms.div_euclid(1000))
        .context("Invalid OData date")
}

pub async fn fetch_timetable(client: &Client) -> Result<Vec<StundenplanItem>> {
    let events: Vec<EventRaw> = odata_list(
        client,
        "ZCM_EM_STUDENT_TIMETABLE_SRV/EventListSet?sap-client=100&$skip=0&$top=5000",
    )
    .await?;

    events.into_iter().map(event_to_item).collect()
}

/// Exam rooms without assignment come as placeholders like "G 0000000020261013".
fn clean_room(room: &str) -> String {
    lazy_static! {
        static ref RE_PLACEHOLDER: Regex = Regex::new(r"^[A-Z] 0{8}\d{8}$").unwrap();
    }
    let room = room.trim();
    if RE_PLACEHOLDER.is_match(room) {
        String::new()
    } else {
        room.to_string()
    }
}

fn event_to_item(e: EventRaw) -> Result<StundenplanItem> {
    let is_exam = e.appointment_type == "EXAM" || !e.exam_category.is_empty();
    let room = clean_room(&e.room);
    let start = odata_date_to_secs(&e.start)?;
    let end = odata_date_to_secs(&e.end)?;
    ensure!(end >= start, "Timetable event ends before it starts");

    let (instructor, description, remarks) = if is_exam {
        let remarks = [e.exam_category.as_str(), e.remarks.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        (e.examiner, e.title.clone(), remarks)
    } else {
        let description = if e.description.is_empty() {
            e.title.clone()
        } else {
            e.description
        };
        (e.lecturer, description, e.remarks)
    };

    // V1 used the short code ("5CS-DPDL-10") as title; frontend and colors rely on it.
    let title = [e.short, e.exam_short]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or(e.title);

    Ok(StundenplanItem {
        all_day: false,
        color: if is_exam {
            "darkred".to_string()
        } else {
            String::new()
        },
        font_color: None,
        description,
        editable: false,
        end,
        instructor: instructor.clone(),
        remarks,
        room: room.clone(),
        sinstructor: instructor,
        sroom: room,
        start,
        title,
    })
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct CourseRaw {
    #[serde(rename = "CourseName")]
    name: String,
    #[serde(rename = "Credits")]
    credits: String,
    #[serde(rename = "Results")]
    results: String,
    #[serde(rename = "StatusCode")]
    status_code: String,
    #[serde(rename = "AcademicYear")]
    academic_year: String,
    #[serde(rename = "Session")]
    session: String,
    #[serde(rename = "ZzAssessmentDate")]
    assessment_date: String,
    #[serde(rename = "ZzAnnouncementDate")]
    announcement_date: String,
}

impl CourseRaw {
    pub fn passed(&self) -> bool {
        self.status_code == "02"
    }

    pub fn credits(&self) -> Result<f64> {
        if self.credits.trim().is_empty() {
            return Ok(0.0);
        }
        let credits: f64 = self
            .credits
            .replace(',', ".")
            .parse()
            .context("Invalid ECTS value")?;
        ensure!(credits.is_finite() && credits >= 0.0, "Invalid ECTS value");
        Ok(credits)
    }

    pub fn is_booked_only(&self) -> bool {
        self.status_code == "01"
    }
}

pub async fn fetch_courses(client: &Client) -> Result<Vec<CourseRaw>> {
    odata_list(
        client,
        "ZPIQ_COURSE_RESULTS_SRV/CourseResultSet?$skip=0&$top=500",
    )
    .await
}

fn is_exam_entry(name: &str) -> bool {
    name.starts_with("P ")
}

/// CourseResultSet lists each module followed by its exam entries ("P ...").
/// Exams are grouped as subgrades of the preceding module.
pub fn courses_to_grades(courses: Vec<CourseRaw>) -> Result<Vec<CampusDualGrade>> {
    let mut grades: Vec<CampusDualGrade> = Vec::new();

    for c in courses {
        let passed = if c.is_booked_only() {
            None
        } else {
            Some(c.passed())
        };
        let akad_period = format!("{} {}", c.academic_year, c.session)
            .trim()
            .to_string();
        let sub = CampusDualSubGrade {
            name: c.name.clone(),
            grade: c.results.clone(),
            passed,
            beurteilung: c.assessment_date.clone(),
            bekanntgabe: c.announcement_date.clone(),
            wiederholung: None,
            akad_period: akad_period.clone(),
            internal_metadata: None,
        };

        if is_exam_entry(&c.name)
            && let Some(module) = grades.last_mut()
        {
            module.subgrades.push(sub);
            continue;
        }

        grades.push(CampusDualGrade {
            name: c.name.clone(),
            grade: c.results.clone(),
            total_passed: passed,
            credit_points: c.credits()?.round() as i32,
            akad_period,
            subgrades: if is_exam_entry(&c.name) {
                vec![sub]
            } else {
                Vec::new()
            },
        });
    }

    // Modules without separate exam entries keep themselves as the only subgrade.
    for g in grades.iter_mut().filter(|g| g.subgrades.is_empty()) {
        g.subgrades.push(CampusDualSubGrade {
            name: g.name.clone(),
            grade: g.grade.clone(),
            passed: g.total_passed,
            beurteilung: String::new(),
            bekanntgabe: String::new(),
            wiederholung: None,
            akad_period: g.akad_period.clone(),
            internal_metadata: None,
        });
    }

    Ok(grades)
}

fn secs_to_local(secs: i64) -> Option<chrono::DateTime<chrono::Local>> {
    chrono::DateTime::from_timestamp(secs, 0).map(|d| d.with_timezone(&chrono::Local))
}

/// V2 has no reminder endpoint; rebuild V1 reminders from results and timetable.
pub async fn fetch_reminders(client: &Client) -> Result<CampusReminders> {
    const LATEST_DAYS: i64 = 30;

    let now = chrono::Local::now();
    let cutoff = (now - chrono::Duration::days(LATEST_DAYS))
        .format("%Y%m%d")
        .to_string();

    let mut latest: Vec<LatestReminder> = fetch_courses(client)
        .await?
        .into_iter()
        .filter(|c| {
            is_exam_entry(&c.name)
                && !c.results.is_empty()
                && c.announcement_date != "00000000"
                && c.announcement_date >= cutoff
        })
        .map(|c| LatestReminder {
            agrdate: c.announcement_date,
            awobject: c.name.trim_start_matches("P ").to_string(),
            awobject_short: c.session.clone(),
            acad_session: c.session,
            acad_year: c.academic_year,
            cpgraded: c.credits,
            gradesymbol: c.results,
            ..Default::default()
        })
        .collect();
    latest.sort_by(|a, b| b.agrdate.cmp(&a.agrdate));

    let now_secs = now.timestamp();
    let mut upcoming: Vec<UpcomingReminder> = fetch_timetable(client)
        .await?
        .into_iter()
        .filter(|e| e.color == "darkred" && e.start >= now_secs)
        .filter_map(|e| {
            let start = secs_to_local(e.start)?;
            let end = secs_to_local(e.end)?;
            Some(UpcomingReminder {
                evdat: start.format("%Y%m%d").to_string(),
                beguz: start.format("%H:%M:%S").to_string(),
                enduz: end.format("%H:%M:%S").to_string(),
                instructor: e.instructor,
                sinstructor: e.sinstructor,
                room: e.room,
                sroom: e.sroom,
                comment: e.remarks,
                sm_short: e.title,
                sm_stext: e.description,
                ..Default::default()
            })
        })
        .collect();
    upcoming.sort_by(|a, b| (&a.evdat, &a.beguz).cmp(&(&b.evdat, &b.beguz)));

    Ok(CampusReminders {
        latest,
        upcoming,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exam_uses_exam_code_and_examiner() {
        let exam = EventRaw {
            title: "P Analysis (K)".into(),
            exam_short: "5CS-MA2AN-00".into(),
            examiner: "Prof. Test".into(),
            appointment_type: "EXAM".into(),
            exam_category: "Klausur, 120 Min.".into(),
            room: "G 0000000020261013".into(),
            start: "/Date(1789974000000)/".into(),
            end: "/Date(1789981200000)/".into(),
            ..Default::default()
        };
        let item = event_to_item(exam).unwrap();
        assert_eq!(item.title, "5CS-MA2AN-00");
        assert_eq!(item.instructor, "Prof. Test");
        assert_eq!(item.description, "P Analysis (K)");
        assert!(item.room.is_empty());
        assert_eq!(item.color, "darkred");
    }

    #[test]
    fn invalid_event_dates_are_not_silently_converted_to_1970() {
        assert!(odata_date_to_secs("not-a-date").is_err());
        assert_eq!(odata_date_to_secs("/Date(1000+0200)/").unwrap(), 1);
        assert!(event_to_item(EventRaw::default()).is_err());
    }

    #[test]
    fn credits_accept_decimal_commas_but_reject_bad_data() {
        let valid = CourseRaw {
            credits: "4,5".into(),
            ..Default::default()
        };
        let invalid = CourseRaw {
            credits: "invalid".into(),
            ..Default::default()
        };
        assert_eq!(valid.credits().unwrap(), 4.5);
        assert!(invalid.credits().is_err());
    }
}

const EXAM_SRV: &str = "ZBA_EXAM_REGISTRATION_SRV";

/// Campus Dual refused a booking or cancellation; the text is safe to show to the user.
#[derive(Debug)]
pub struct ExamRejected(pub String);

impl std::fmt::Display for ExamRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExamRejected {}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PeriodRaw {
    peryr: String,
    perid: String,
    #[serde(default)]
    period_title: String,
    #[serde(default)]
    default_period: String,
}

#[derive(Deserialize)]
struct OffersRaw {
    #[serde(default)]
    results: Vec<ExamOfferRaw>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExamOfferRaw {
    #[serde(default)]
    offerno: String,
    #[serde(default)]
    ce_objid: Option<String>,
    #[serde(default)]
    exam_date: Option<String>,
    #[serde(default)]
    exam_begin_time: Option<String>,
    #[serde(default)]
    exam_end_time: Option<String>,
    #[serde(default)]
    room_short: String,
    #[serde(default)]
    exam_mode_text: String,
    #[serde(default)]
    examiner: String,
    #[serde(default)]
    regis_end: Option<String>,
    #[serde(default)]
    dereg_end: Option<String>,
    #[serde(default)]
    booked_flag: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExamRaw {
    #[serde(default)]
    sc_objid: String,
    #[serde(default)]
    sm_tlst_objid: String,
    #[serde(default)]
    sm_tlst_descr: String,
    #[serde(default)]
    sm_clst_descr: String,
    #[serde(default)]
    sm_clst_short: String,
    #[serde(default)]
    registration_state: String,
    #[serde(default)]
    to_exam_offers: Option<OffersRaw>,
}

fn is_sap_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 16 && value.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn date_label(value: Option<&str>) -> String {
    value
        .and_then(|raw| odata_date_to_secs(raw).ok())
        .filter(|secs| *secs > 0)
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .map(|date| date.format("%d.%m.%Y").to_string())
        .unwrap_or_default()
}

/// "PT10H30M00S" -> "10:30"
fn time_label(value: Option<&str>) -> Option<String> {
    lazy_static! {
        static ref RE_TIME: Regex = Regex::new(r"^PT(\d+)H(\d+)M").unwrap();
    }
    let captures = RE_TIME.captures(value?)?;
    Some(format!("{:0>2}:{}", &captures[1], &captures[2]))
}

pub async fn fetch_exam_periods(client: &Client) -> Result<Vec<ExamPeriod>> {
    let periods: Vec<PeriodRaw> = odata_list(
        client,
        &format!("{EXAM_SRV}/PeriodSet?sap-client=100&$skip=0&$top=100"),
    )
    .await?;
    Ok(periods
        .into_iter()
        .map(|p| ExamPeriod {
            year: p.peryr,
            id: p.perid,
            title: p.period_title,
            is_default: p.default_period == "X",
        })
        .collect())
}

pub async fn fetch_exams(client: &Client, year: &str, period: &str) -> Result<Vec<ExamEntry>> {
    ensure!(is_sap_id(year) && is_sap_id(period), "Invalid period");
    let exams: Vec<ExamRaw> = odata_list(
        client,
        &format!(
            "{EXAM_SRV}/ExamSet?sap-client=100&$skip=0&$top=100&$expand=ToExamOffers&$filter=Perid%20eq%20%27{period}%27%20and%20Peryr%20eq%20%27{year}%27"
        ),
    )
    .await?;
    Ok(exams
        .into_iter()
        .map(|e| {
            let name = if e.sm_tlst_descr.is_empty() {
                e.sm_clst_descr
            } else {
                e.sm_tlst_descr
            };
            let offers = e
                .to_exam_offers
                .map(|o| o.results)
                .unwrap_or_default()
                .into_iter()
                .map(|o| {
                    let begin = time_label(o.exam_begin_time.as_deref());
                    let end = time_label(o.exam_end_time.as_deref());
                    ExamOffer {
                        offer_no: o.offerno,
                        ce_objid: o.ce_objid.unwrap_or_default(),
                        date: date_label(o.exam_date.as_deref()),
                        time: match (begin, end) {
                            (Some(b), Some(e)) => format!("{b} - {e}"),
                            (Some(b), None) => b,
                            _ => String::new(),
                        },
                        room: o.room_short,
                        mode: o.exam_mode_text,
                        examiner: o.examiner,
                        registration_until: date_label(o.regis_end.as_deref()),
                        deregistration_until: date_label(o.dereg_end.as_deref()),
                        booked: o.booked_flag,
                    }
                })
                .collect();
            ExamEntry {
                sm_tlst_objid: e.sm_tlst_objid,
                sc_objid: e.sc_objid,
                year: year.to_string(),
                period: period.to_string(),
                name,
                short: e.sm_clst_short,
                registration_state: e.registration_state,
                offers,
            }
        })
        .collect())
}

const DOCUMENTS_SRV: &str = "ZBA_CM_STUDENT_DOCUMENTS_SRV";
const OPT_MODULES_SRV: &str = "ZBA_OPT_MODULES_SRV";

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FormRaw {
    formname: String,
    #[serde(default)]
    form_title: String,
    student_id: String,
    creation_date: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SemesterRaw {
    peryr: String,
    perid: String,
    #[serde(default)]
    period_title: String,
    #[serde(default)]
    registration_is_open: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OptModuleRaw {
    #[serde(default)]
    registration_description: String,
    #[serde(default)]
    description_primary: String,
    #[serde(default)]
    description_secondary: String,
    #[serde(default)]
    target_semester: serde_json::Value,
    #[serde(default)]
    free_capacity: serde_json::Value,
    #[serde(default)]
    status: String,
    #[serde(default)]
    show_register_btn: serde_json::Value,
    #[serde(default)]
    enable_register_btn: serde_json::Value,
    #[serde(default)]
    show_deregister_btn: serde_json::Value,
    #[serde(default)]
    enable_deregister_btn: serde_json::Value,
}

fn flag(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::String(s) => matches!(s.as_str(), "X" | "x" | "true" | "1"),
        _ => false,
    }
}

fn loose_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// "/Date(ms)/" -> "2026-11-13"
fn iso_date(value: &str) -> Option<String> {
    let secs = odata_date_to_secs(value).ok()?;
    Some(
        chrono::DateTime::from_timestamp(secs, 0)?
            .format("%Y-%m-%d")
            .to_string(),
    )
}

pub async fn fetch_documents(client: &Client) -> Result<Documents> {
    let forms: Vec<FormRaw> = odata_list(
        client,
        &format!("{DOCUMENTS_SRV}/FormSet?sap-client=100&$skip=0&$top=100&$select=StudentId,CreationDate,Formname,FormTitle,Peryr,Perid"),
    )
    .await?;
    let semesters: Vec<SemesterRaw> = odata_list(
        client,
        &format!("{DOCUMENTS_SRV}/SemesterSet?sap-client=100&$skip=0&$top=100"),
    )
    .await?;
    Ok(Documents {
        forms: forms
            .into_iter()
            .filter_map(|form| {
                Some(DocumentForm {
                    created: iso_date(&form.creation_date)?,
                    name: form.formname,
                    title: form.form_title,
                    student_id: form.student_id,
                })
            })
            .collect(),
        semesters: semesters
            .into_iter()
            .map(|s| DocumentSemester {
                year: s.peryr,
                id: s.perid,
                title: s.period_title,
            })
            .collect(),
    })
}

#[derive(Deserialize)]
pub struct DocumentRequest {
    pub form: String,
    pub student: String,
    pub created: String,
    pub year: String,
    pub period: String,
}

/// Downloads a generated document; returns the PDF bytes.
pub async fn fetch_document_pdf(client: &Client, request: &DocumentRequest) -> Result<Vec<u8>> {
    fetch_document_pdf_at(client, ODATA_BASE, request).await
}

async fn fetch_document_pdf_at(
    client: &Client,
    base: &str,
    request: &DocumentRequest,
) -> Result<Vec<u8>> {
    let valid_form = !request.form.is_empty()
        && request
            .form
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_');
    ensure!(
        valid_form
            && is_sap_id(&request.student)
            && is_sap_id(&request.year)
            && is_sap_id(&request.period)
            && chrono::NaiveDate::parse_from_str(&request.created, "%Y-%m-%d").is_ok(),
        "Invalid document parameter"
    );
    let form = urlencoding_form(&request.form);
    let url = format!(
        "{base}/{DOCUMENTS_SRV}/PrintDocumentSet(Formname='{form}',StudentId='{}',CreationDate=datetime%27{}T00%3A00%3A00%27,Peryr='{}',Perid='{}')/$value?sap-client=100",
        request.student, request.created, request.year, request.period
    );
    let response = client.get(&url).send().await?.error_for_status()?;
    let is_pdf = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().contains("pdf"));
    ensure!(
        is_pdf,
        "Campus Dual did not return a PDF (session expired?)"
    );
    Ok(response.bytes().await?.to_vec())
}

fn urlencoding_form(name: &str) -> String {
    name.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b == b'_' {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

pub async fn fetch_opt_modules(client: &Client) -> Result<OptModules> {
    let semesters: Vec<SemesterRaw> = odata_list(
        client,
        &format!("{OPT_MODULES_SRV}/SemesterSet?sap-client=100&$skip=0&$top=100"),
    )
    .await?;
    let mut periods = Vec::new();
    for semester in semesters {
        let open = semester.registration_is_open == "X";
        let modules = if open {
            ensure!(
                is_sap_id(&semester.peryr) && is_sap_id(&semester.perid),
                "Invalid period"
            );
            let raw: Vec<OptModuleRaw> = odata_list(
                client,
                &format!(
                    "{OPT_MODULES_SRV}/OptModuleSet?sap-client=100&$skip=0&$top=100&$orderby=RegistrationDescription%20asc&$filter=Peryr%20eq%20%27{}%27%20and%20Perid%20eq%20%27{}%27",
                    semester.peryr, semester.perid
                ),
            )
            .await?;
            raw.into_iter()
                .map(|m| OptModule {
                    group: m.registration_description,
                    title: m.description_primary,
                    subtitle: m.description_secondary,
                    semester: loose_text(&m.target_semester),
                    free_capacity: loose_text(&m.free_capacity),
                    status: m.status,
                    can_register: flag(&m.show_register_btn) && flag(&m.enable_register_btn),
                    can_deregister: flag(&m.show_deregister_btn) && flag(&m.enable_deregister_btn),
                })
                .collect()
        } else {
            Vec::new()
        };
        periods.push(OptPeriod {
            title: semester.period_title,
            open,
            modules,
        });
    }
    Ok(OptModules { periods })
}

fn find_message(value: &serde_json::Value) -> Option<(String, String)> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(text)) = map.get("Message") {
                let kind = map
                    .get("MessageType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                return Some((kind, text.clone()));
            }
            map.values().find_map(find_message)
        }
        serde_json::Value::Array(items) => items.iter().find_map(find_message),
        _ => None,
    }
}

/// Calls the bookExam/cancelExam function import (declared as HTTP GET in the metadata).
pub async fn exam_action(
    client: &Client,
    book: bool,
    request: &ExamActionRequest,
) -> Result<String> {
    exam_action_at(client, ODATA_BASE, book, request).await
}

fn validate_exam_request(request: &ExamActionRequest) -> Result<()> {
    for id in [
        &request.sm_tlst_objid,
        &request.year,
        &request.period,
        &request.offer_no,
        &request.sc_objid,
        &request.ce_objid,
    ] {
        ensure!(is_sap_id(id), "Invalid exam parameter");
    }
    Ok(())
}

async fn exam_action_at(
    client: &Client,
    base: &str,
    book: bool,
    request: &ExamActionRequest,
) -> Result<String> {
    validate_exam_request(request)?;
    let function = if book { "bookExam" } else { "cancelExam" };
    let text = odata_get_text_at(
        client,
        base,
        &format!(
            "{EXAM_SRV}/{function}?sap-client=100&SmTlstObjid=%27{}%27&Peryr=%27{}%27&Perid=%27{}%27&ExamOfferNo=%27{}%27&ScObjid=%27{}%27&CeObjid=%27{}%27",
            request.sm_tlst_objid,
            request.year,
            request.period,
            request.offer_no,
            request.sc_objid,
            request.ce_objid
        ),
    )
    .await?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    let (kind, message) = find_message(&value).unwrap_or_default();
    if matches!(kind.as_str(), "E" | "A") {
        return Err(ExamRejected(if message.is_empty() {
            "Campus Dual hat die Anfrage abgelehnt.".to_string()
        } else {
            message
        })
        .into());
    }
    Ok(message)
}

#[cfg(test)]
mod exam_tests {
    use super::*;

    #[test]
    fn formats_sap_dates_and_times() {
        assert_eq!(time_label(Some("PT09H30M00S")).as_deref(), Some("09:30"));
        assert_eq!(date_label(Some("/Date(1782691200000)/")), "29.06.2026");
        assert_eq!(date_label(None), "");
    }

    #[test]
    fn finds_nested_message_and_rejects_unsafe_ids() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"d":{"results":[{"MessageType":"E","Message":"Frist abgelaufen"}]}}"#,
        )
        .unwrap();
        assert_eq!(
            find_message(&value),
            Some(("E".into(), "Frist abgelaufen".into()))
        );
        assert!(is_sap_id("00012345"));
        assert!(!is_sap_id("1' or '1"));
    }

    /// Serves one canned response on localhost and returns the base URL plus the received request line.
    async fn mock_server(body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0_u8; 4096];
            let read = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            request.lines().next().unwrap_or_default().to_string()
        });
        (base, handle)
    }

    fn sample_request() -> ExamActionRequest {
        ExamActionRequest {
            sm_tlst_objid: "00012345".into(),
            year: "2026".into(),
            period: "001".into(),
            offer_no: "0001".into(),
            sc_objid: "00098765".into(),
            ce_objid: "00055555".into(),
        }
    }

    #[tokio::test]
    async fn cancel_calls_cancel_exam_against_mock_backend() {
        let (base, handle) =
            mock_server(r#"{"d":{"MessageType":"S","Message":"Abgemeldet"}}"#).await;
        let message = exam_action_at(&reqwest::Client::new(), &base, false, &sample_request())
            .await
            .unwrap();
        assert_eq!(message, "Abgemeldet");
        let request_line = handle.await.unwrap();
        assert!(request_line.contains("/ZBA_EXAM_REGISTRATION_SRV/cancelExam?"));
        assert!(request_line.contains("ExamOfferNo=%270001%27"));
    }

    #[tokio::test]
    async fn book_calls_book_exam_and_reports_rejection() {
        let (base, handle) =
            mock_server(r#"{"d":{"MessageType":"E","Message":"Frist abgelaufen"}}"#).await;
        let error = exam_action_at(&reqwest::Client::new(), &base, true, &sample_request())
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "Frist abgelaufen");
        assert!(handle.await.unwrap().contains("/bookExam?"));
    }

    #[tokio::test]
    async fn invalid_ids_never_reach_the_backend() {
        let mut request = sample_request();
        request.offer_no = "1' or '1".into();
        let result = exam_action_at(
            &reqwest::Client::new(),
            "http://127.0.0.1:1",
            false,
            &request,
        )
        .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid exam parameter")
        );
    }

    #[test]
    fn opt_module_flags_and_dates() {
        assert!(flag(&serde_json::json!("X")));
        assert!(flag(&serde_json::json!(true)));
        assert!(!flag(&serde_json::json!("")));
        assert_eq!(loose_text(&serde_json::json!(12)), "12");
        assert_eq!(
            iso_date("/Date(1794528000000)/").as_deref(),
            Some("2026-11-13")
        );
    }

    #[tokio::test]
    async fn document_pdf_is_requested_with_selected_semester() {
        let (base, handle) = mock_server("%PDF-1.3").await;
        let request = DocumentRequest {
            form: "STUDIEN_BESCH".into(),
            student: "91023845".into(),
            created: "2026-11-13".into(),
            year: "2027".into(),
            period: "001".into(),
        };
        // The mock answers with JSON, so a PDF is rejected rather than passed on.
        let result = fetch_document_pdf_at(&reqwest::Client::new(), &base, &request).await;
        assert!(result.is_err());
        let line = handle.await.unwrap();
        assert!(line.contains("PrintDocumentSet(Formname='STUDIEN_BESCH'"));
        assert!(line.contains("Peryr='2027',Perid='001'"));
    }

    #[tokio::test]
    async fn document_requests_reject_unsafe_input() {
        let request = DocumentRequest {
            form: "x')/../".into(),
            student: "1".into(),
            created: "2026-11-13".into(),
            year: "2027".into(),
            period: "001".into(),
        };
        let result =
            fetch_document_pdf_at(&reqwest::Client::new(), "http://127.0.0.1:1", &request).await;
        assert!(result.unwrap_err().to_string().contains("Invalid document"));
    }
}
