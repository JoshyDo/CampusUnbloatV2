pub use campusunbloat_shared::{
    CampusDualGrade, CampusDualSubGrade, CampusReminders, DocumentForm, DocumentSemester,
    Documents, ExamActionRequest, ExamEntry, ExamOffer, ExamPeriod, LatestReminder, OptModule,
    OptModules, OptPeriod, StundenplanItem, UpcomingReminder, UserBasicInfo,
};

pub struct CampusLoginData {
    pub username: String,
    pub password: String,
}

#[derive(Clone)]
pub struct CdAuthData {
    pub cookie: String,
    pub user: String,
    pub password: String,
}
