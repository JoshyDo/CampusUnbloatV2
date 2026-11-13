use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Extension, Json,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use campusunbloat_shared::{LoginRequest, UserBasicInfo};
use rand::RngExt;
use tokio::sync::{Mutex, RwLock};

use crate::{
    campus_backend::{
        login::cdlogin_get_jcookie_and_meta,
        odata::{client_from_cookies, session_alive},
    },
    error::AppError,
    remember,
    types::{CampusLoginData, CdAuthData},
};

const COOKIE_NAME: &str = "campus_session";
const REMEMBER_COOKIE: &str = "campus_remember";
const SESSION_TTL: Duration = Duration::from_secs(8 * 60 * 60);
const REMEMBER_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const SESSION_CHECK_INTERVAL: Duration = Duration::from_secs(60);
const LOGIN_WINDOW: Duration = Duration::from_secs(60);
const LOGIN_LIMIT: usize = 10;
const MAX_SESSIONS: usize = 64;

#[derive(Clone, Default)]
pub struct AppState {
    sessions: Arc<RwLock<HashMap<String, Arc<StoredSession>>>>,
    login_attempts: Arc<Mutex<VecDeque<Instant>>>,
    restore_lock: Arc<Mutex<()>>,
}

struct StoredSession {
    expires_at: Instant,
    data: Mutex<SessionData>,
}

struct SessionData {
    auth: CdAuthData,
    user: UserBasicInfo,
    checked_at: Instant,
}

#[derive(Clone)]
pub struct SessionContext {
    pub auth: CdAuthData,
    pub user: UserBasicInfo,
}

impl AppState {
    pub async fn remove_expired(&self) {
        let now = Instant::now();
        self.sessions
            .write()
            .await
            .retain(|_, session| session.expires_at > now);
    }

    async fn allow_login(&self) -> Result<(), AppError> {
        let now = Instant::now();
        let mut attempts = self.login_attempts.lock().await;
        while attempts
            .front()
            .is_some_and(|attempt| now.duration_since(*attempt) >= LOGIN_WINDOW)
        {
            attempts.pop_front();
        }
        if attempts.len() >= LOGIN_LIMIT {
            return Err(AppError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "Zu viele Anmeldeversuche. Bitte warte eine Minute.",
            ));
        }
        attempts.push_back(now);
        Ok(())
    }
}

fn new_session_id() -> String {
    let mut random = [0_u8; 32];
    rand::rng().fill(&mut random);
    random.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .flat_map(|header| header.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then(|| value.to_string()))
}

fn session_id(headers: &HeaderMap) -> Option<String> {
    cookie_value(headers, COOKIE_NAME)
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn cookie_header(name: &str, value: &str, max_age: u64) -> Result<HeaderValue, AppError> {
    let secure = if std::env::var_os("CAMPUSUNBLOAT_SECURE_COOKIES").is_some() {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"
    ))
    .map_err(|error| {
        log::error!("Could not construct session cookie: {error}");
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Die Sitzung konnte nicht erstellt werden.",
        )
    })
}
fn login_error(error: anyhow::Error) -> AppError {
    let invalid_credentials = error
        .chain()
        .any(|cause| cause.to_string().contains("SAMLResponse missing"));
    log::warn!("Campus Dual login failed: {error:#}");
    if invalid_credentials {
        AppError::new(
            StatusCode::UNAUTHORIZED,
            "Die Anmeldung wurde abgelehnt. Bitte prüfe deine Zugangsdaten.",
        )
    } else {
        AppError::new(
            StatusCode::BAD_GATEWAY,
            "Die Anmeldung bei Campus Dual ist zurzeit nicht möglich.",
        )
    }
}

pub async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let username = request.username.trim();
    if username.is_empty()
        || username.len() > 256
        || request.password.is_empty()
        || request.password.len() > 1024
    {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "Bitte gib einen gültigen Benutzernamen und ein Passwort ein.",
        ));
    }
    state.allow_login().await?;
    state.remove_expired().await;
    if state.sessions.read().await.len() >= MAX_SESSIONS {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Es sind bereits zu viele Sitzungen aktiv.",
        ));
    }
    let (auth, user) = cdlogin_get_jcookie_and_meta(CampusLoginData {
        username: username.to_string(),
        password: request.password.clone(),
    })
    .await
    .map_err(login_error)?;

    let session = new_stored_session(auth, user.clone());
    let id = new_session_id();
    let cookie = cookie_header(COOKIE_NAME, &id, SESSION_TTL.as_secs())?;
    let remember_cookie = if request.remember {
        match remember::seal(username, &request.password, REMEMBER_TTL.as_secs()) {
            Ok(token) => Some(cookie_header(
                REMEMBER_COOKIE,
                &token,
                REMEMBER_TTL.as_secs(),
            )?),
            Err(error) => {
                log::warn!("Login could not be remembered: {error:#}");
                None
            }
        }
    } else {
        None
    };
    let mut sessions = state.sessions.write().await;
    if sessions.len() >= MAX_SESSIONS {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Es sind bereits zu viele Sitzungen aktiv.",
        ));
    }
    sessions.insert(id, session);

    let mut headers = HeaderMap::new();
    headers.append(header::SET_COOKIE, cookie);
    if let Some(remember_cookie) = remember_cookie {
        headers.append(header::SET_COOKIE, remember_cookie);
    }
    Ok((headers, Json(user)))
}

fn new_stored_session(auth: CdAuthData, user: UserBasicInfo) -> Arc<StoredSession> {
    Arc::new(StoredSession {
        expires_at: Instant::now() + SESSION_TTL,
        data: Mutex::new(SessionData {
            auth,
            user,
            checked_at: Instant::now(),
        }),
    })
}

/// Rebuilds a session from the sealed remember cookie (new browser session,
/// server restart or expiry).
async fn restore_session(state: &AppState, id: &str, token: &str) -> Option<Arc<StoredSession>> {
    let _guard = state.restore_lock.lock().await;
    if let Some(session) = state.sessions.read().await.get(id).cloned()
        && session.expires_at > Instant::now()
    {
        return Some(session);
    }
    let (username, password) = remember::open(token)?;
    state.allow_login().await.ok()?;
    match cdlogin_get_jcookie_and_meta(CampusLoginData { username, password }).await {
        Ok((auth, user)) => {
            let session = new_stored_session(auth, user);
            state
                .sessions
                .write()
                .await
                .insert(id.to_string(), session.clone());
            Some(session)
        }
        Err(error) => {
            log::warn!("Remembered login failed: {error:#}");
            None
        }
    }
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    if let Some(id) = session_id(&headers) {
        state.sessions.write().await.remove(&id);
    }
    let mut response_headers = HeaderMap::new();
    response_headers.append(header::SET_COOKIE, cookie_header(COOKIE_NAME, "", 0)?);
    response_headers.append(header::SET_COOKIE, cookie_header(REMEMBER_COOKIE, "", 0)?);
    Ok((response_headers, StatusCode::NO_CONTENT))
}

pub async fn require_session(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let remember_token = cookie_value(request.headers(), REMEMBER_COOKIE);
    let mut new_cookie = None;
    let id = match session_id(request.headers()) {
        Some(id) => id,
        None if remember_token.is_some() => {
            let id = new_session_id();
            new_cookie = Some(cookie_header(COOKIE_NAME, &id, SESSION_TTL.as_secs())?);
            id
        }
        None => return Err(AppError::unauthorized()),
    };
    let existing = state.sessions.read().await.get(&id).cloned();
    let session = match existing {
        Some(session) if session.expires_at > Instant::now() => session,
        _ => {
            state.sessions.write().await.remove(&id);
            let token = remember_token
                .as_deref()
                .ok_or_else(AppError::unauthorized)?;
            restore_session(&state, &id, token)
                .await
                .ok_or_else(AppError::unauthorized)?
        }
    };
    let mut data = session.data.lock().await;
    if data.checked_at.elapsed() >= SESSION_CHECK_INTERVAL {
        let client = client_from_cookies(&data.auth.cookie)?;
        if !session_alive(&client).await? {
            let login = CampusLoginData {
                username: data.auth.user.clone(),
                password: data.auth.password.clone(),
            };
            match cdlogin_get_jcookie_and_meta(login).await {
                Ok((auth, user)) => {
                    data.auth = auth;
                    data.user = user;
                }
                Err(error) => {
                    log::warn!("Campus Dual session could not be renewed: {error:#}");
                    drop(data);
                    state.sessions.write().await.remove(&id);
                    return Err(AppError::unauthorized());
                }
            }
        }
        data.checked_at = Instant::now();
    }
    let context = SessionContext {
        auth: data.auth.clone(),
        user: data.user.clone(),
    };
    drop(data);
    request.extensions_mut().insert(context);
    let mut response = next.run(request).await;
    if let Some(cookie) = new_cookie {
        response.headers_mut().append(header::SET_COOKIE, cookie);
    }
    Ok(response)
}

pub async fn current_user(Extension(session): Extension<SessionContext>) -> Json<UserBasicInfo> {
    Json(session.user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_contains_only_an_opaque_random_identifier() {
        let id = new_session_id();
        assert_eq!(id.len(), 64);
        let cookie = cookie_header(COOKIE_NAME, &id, SESSION_TTL.as_secs()).unwrap();
        let cookie = cookie.to_str().unwrap();
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(!cookie.contains("password"));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{COOKIE_NAME}={id}")).unwrap(),
        );
        assert_eq!(session_id(&headers).as_deref(), Some(id.as_str()));
    }

    #[tokio::test]
    async fn expired_sessions_are_removed() {
        let state = AppState::default();
        state.sessions.write().await.insert(
            new_session_id(),
            Arc::new(StoredSession {
                expires_at: Instant::now(),
                data: Mutex::new(SessionData {
                    auth: CdAuthData {
                        cookie: String::new(),
                        user: String::new(),
                        password: String::new(),
                    },
                    user: UserBasicInfo::default(),
                    checked_at: Instant::now(),
                }),
            }),
        );
        state.remove_expired().await;
        assert!(state.sessions.read().await.is_empty());
    }
}
