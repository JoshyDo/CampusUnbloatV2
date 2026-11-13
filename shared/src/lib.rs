use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub remember: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudyProgress {
    pub semester: u32,
    pub total_semesters: u32,
    pub ects: f64,
    pub possible_ects: f64,
    pub degree_ects: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UserBasicInfo {
    pub first_name: String,
    pub last_name: String,
    pub seminar_group: String,
    pub seminar_name: String,
    pub user: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CampusDualGrade {
    pub name: String,
    pub grade: String,
    pub total_passed: Option<bool>,
    pub credit_points: i32,
    pub akad_period: String,
    pub subgrades: Vec<CampusDualSubGrade>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CampusDualSubGrade {
    pub name: String,
    pub grade: String,
    pub passed: Option<bool>,
    pub beurteilung: String,
    pub bekanntgabe: String,
    pub wiederholung: Option<String>,
    pub akad_period: String,
    pub internal_metadata: Option<SubGradeMetadata>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubGradeMetadata {
    pub module: String,
    pub peryr: String,
    pub perid: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StundenplanItem {
    #[serde(rename = "allDay")]
    pub all_day: bool,
    pub color: String,
    pub font_color: Option<String>,
    pub description: String,
    pub editable: bool,
    pub end: i64,
    pub instructor: String,
    pub remarks: String,
    pub room: String,
    pub sinstructor: String,
    pub sroom: String,
    pub start: i64,
    pub title: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LatestReminder {
    pub acad_session: String,
    pub acad_year: String,
    pub agrdate: String,
    pub agrtype: String,
    pub awobject: String,
    pub awobject_short: String,
    pub awotype: String,
    pub awstatus: String,
    pub bookdate: String,
    pub bookreason: String,
    pub cpgraded: String,
    pub cpunit: String,
    pub gradesymbol: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UpcomingReminder {
    pub beguz: String,
    pub comment: String,
    pub enduz: String,
    pub evdat: String,
    pub instructor: String,
    pub location: String,
    pub objid: String,
    pub room: String,
    pub sinstructor: String,
    pub sm_short: String,
    pub sm_stext: String,
    pub sroom: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CampusReminders {
    pub electives: i64,
    pub exams: i64,
    pub latest: Vec<LatestReminder>,
    pub semester: i64,
    pub upcoming: Vec<UpcomingReminder>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Canteen {
    pub id: u32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MealGroup {
    pub meal_type: String,
    pub sub_meals: Vec<SingleMeal>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SingleMeal {
    pub name: String,
    pub additional_ingredients: Vec<String>,
    pub allergens: Option<String>,
    pub variations: Option<Vec<MealVariation>>,
    pub price: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MealVariation {
    pub name: String,
    pub allergens_and_add: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExamPeriod {
    pub year: String,
    pub id: String,
    pub title: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExamOffer {
    pub offer_no: String,
    pub ce_objid: String,
    pub date: String,
    pub time: String,
    pub room: String,
    pub mode: String,
    pub examiner: String,
    pub registration_until: String,
    pub deregistration_until: String,
    pub booked: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExamEntry {
    pub sm_tlst_objid: String,
    pub sc_objid: String,
    pub year: String,
    pub period: String,
    pub name: String,
    pub short: String,
    pub registration_state: String,
    pub offers: Vec<ExamOffer>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExamActionRequest {
    pub sm_tlst_objid: String,
    pub year: String,
    pub period: String,
    pub offer_no: String,
    pub sc_objid: String,
    pub ce_objid: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExamActionResult {
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentForm {
    pub name: String,
    pub title: String,
    pub student_id: String,
    /// ISO date (YYYY-MM-DD) as required to request the PDF.
    pub created: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentSemester {
    pub year: String,
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Documents {
    pub forms: Vec<DocumentForm>,
    pub semesters: Vec<DocumentSemester>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OptModule {
    pub group: String,
    pub title: String,
    pub subtitle: String,
    pub semester: String,
    pub free_capacity: String,
    pub status: String,
    pub can_register: bool,
    pub can_deregister: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OptPeriod {
    pub title: String,
    pub open: bool,
    pub modules: Vec<OptModule>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OptModules {
    pub periods: Vec<OptPeriod>,
}
