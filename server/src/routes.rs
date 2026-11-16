use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use tower_http::services::ServeDir;

use crate::{
    auth::{self, AppState},
    services,
};

#[derive(Clone)]
struct LocalOrigin {
    port: u16,
    content_security_policy: HeaderValue,
    secure_cookies: bool,
    /// An additional host (e.g. `campus.example.com`) that is allowed in the
    /// `Host`/`Origin` headers, for setups hosted behind a reverse proxy
    /// under a real domain. Configured via `CAMPUSUNBLOAT_ALLOWED_HOST`.
    allowed_host: Option<String>,
}

pub fn content_security_policy(web_dir: &Path) -> Result<HeaderValue> {
    let index = std::fs::read_to_string(web_dir.join("index.html"))
        .context("Could not read the built web application")?;
    policy_for_html(&index)
}

fn policy_for_html(index: &str) -> Result<HeaderValue> {
    let html = Html::parse_document(index);
    let selector = Selector::parse("script").expect("valid script selector");
    let mut scripts = String::from("'self' 'wasm-unsafe-eval'");
    for script in html
        .select(&selector)
        .filter(|script| script.attr("src").is_none())
    {
        let hash = STANDARD.encode(Sha256::digest(script.inner_html().as_bytes()));
        scripts.push_str(&format!(" 'sha256-{hash}'"));
    }
    HeaderValue::from_str(&format!(
        "default-src 'self'; connect-src 'self'; script-src {scripts}; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'"
    ))
    .context("Could not construct the web application's security policy")
}

pub fn app(
    state: AppState,
    mensa: Router,
    web_dir: PathBuf,
    port: u16,
    content_security_policy: HeaderValue,
) -> Router {
    let protected = Router::new()
        .route("/session", get(auth::current_user))
        .route("/grades", get(services::grades))
        .route("/progress", get(services::progress))
        .route("/ects", get(services::ects))
        .route("/fachsemester", get(services::fachsemester))
        .route("/timetable", get(services::timetable))
        .route("/reminders", get(services::reminders))
        .route("/documents", get(services::documents))
        .route("/documents/pdf", get(services::document_pdf))
        .route("/opt-modules", get(services::opt_modules))
        .route("/exams/periods", get(services::exam_periods))
        .route("/exams", get(services::exams))
        .route("/exams/open", get(services::open_exams))
        .route("/exams/book", post(services::book_exam))
        .route("/exams/cancel", post(services::cancel_exam))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ));
    let api: Router = Router::new()
        .route("/login", post(auth::login))
        .route("/logout", post(auth::logout))
        .merge(protected)
        .with_state(state);
    let api = api.nest("/mensa", mensa);

    Router::new()
        .nest("/api", api)
        .fallback_service(ServeDir::new(web_dir).append_index_html_on_directories(true))
        .layer(middleware::from_fn_with_state(
            LocalOrigin {
                port,
                content_security_policy,
                secure_cookies: std::env::var_os("CAMPUSUNBLOAT_SECURE_COOKIES").is_some(),
                allowed_host: std::env::var("CAMPUSUNBLOAT_ALLOWED_HOST").ok(),
            },
            local_only,
        ))
}

async fn local_only(State(local): State<LocalOrigin>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let is_local_host = host.eq_ignore_ascii_case(&format!("localhost:{}", local.port))
        || host.eq_ignore_ascii_case(&format!("127.0.0.1:{}", local.port));
    let is_allowed_host = local
        .allowed_host
        .as_deref()
        .is_some_and(|allowed| host.eq_ignore_ascii_case(allowed));
    if !is_local_host && !is_allowed_host {
        return (StatusCode::FORBIDDEN, "Ungültiger Host.").into_response();
    }
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        let origin = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        // Accept either scheme: a reverse proxy may terminate TLS and forward
        // to this server over plain HTTP, while the browser's Origin header
        // still reflects the public HTTPS URL the user actually visited.
        let origin_matches = origin.eq_ignore_ascii_case(&format!("http://{host}"))
            || origin.eq_ignore_ascii_case(&format!("https://{host}"));
        if !origin_matches {
            return (StatusCode::FORBIDDEN, "Ungültiger Ursprung.").into_response();
        }
    }
    let is_api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        local.content_security_policy,
    );
    if local.secure_cookies {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
    if is_api {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    fn test_app() -> Router {
        app(
            AppState::default(),
            Router::new(),
            PathBuf::from("missing"),
            3000,
            HeaderValue::from_static("default-src 'self'"),
        )
    }

    #[tokio::test]
    async fn hsts_header_is_only_sent_when_secure_cookies_are_enabled() {
        let request = || {
            Request::builder()
                .uri("/api/session")
                .header(header::HOST, "localhost:3000")
                .body(Body::empty())
                .unwrap()
        };
        let response = test_app().oneshot(request()).await.unwrap();
        assert!(
            !response
                .headers()
                .contains_key(header::STRICT_TRANSPORT_SECURITY)
        );

        // SAFETY: this test is the only one reading CAMPUSUNBLOAT_SECURE_COOKIES and
        // restores it immediately after building the router.
        unsafe { std::env::set_var("CAMPUSUNBLOAT_SECURE_COOKIES", "1") };
        let secure_app = test_app();
        unsafe { std::env::remove_var("CAMPUSUNBLOAT_SECURE_COOKIES") };
        let response = secure_app.oneshot(request()).await.unwrap();
        assert_eq!(
            response.headers()[header::STRICT_TRANSPORT_SECURITY],
            "max-age=31536000; includeSubDomains"
        );
    }

    #[tokio::test]
    async fn allowed_host_can_be_configured_for_hosted_deployments() {
        let request = Request::builder()
            .uri("/api/session")
            .header(header::HOST, "campus.example.com")
            .body(Body::empty())
            .unwrap();
        // SAFETY: this test is the only one reading CAMPUSUNBLOAT_ALLOWED_HOST and
        // restores it immediately after building the router.
        unsafe { std::env::set_var("CAMPUSUNBLOAT_ALLOWED_HOST", "campus.example.com") };
        let hosted_app = test_app();
        unsafe { std::env::remove_var("CAMPUSUNBLOAT_ALLOWED_HOST") };
        assert_eq!(
            hosted_app.oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn security_policy_allows_only_the_built_inline_scripts() {
        let code = "\nconsole.log('ready');\n";
        let policy = policy_for_html(&format!(
            "<script type=\"module\">{code}</script><script src=\"external.js\"></script>"
        ))
        .unwrap();
        let policy = policy.to_str().unwrap();
        let expected = STANDARD.encode(Sha256::digest(code.as_bytes()));
        assert!(policy.contains(&format!("'sha256-{expected}'")));
        let script_policy = policy
            .split(';')
            .find(|part| part.contains("script-src"))
            .unwrap();
        assert!(!script_policy.contains("'unsafe-inline'"));
        assert_eq!(script_policy.matches("sha256-").count(), 1);
    }

    #[tokio::test]
    async fn blocks_untrusted_hosts_and_cross_origin_posts() {
        let foreign_host = Request::builder()
            .uri("/api/session")
            .header(header::HOST, "example.com:3000")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            test_app().oneshot(foreign_host).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );

        let foreign_origin = Request::builder()
            .method(Method::POST)
            .uri("/api/logout")
            .header(header::HOST, "localhost:3000")
            .header(header::ORIGIN, "https://example.com")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            test_app().oneshot(foreign_origin).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn protected_routes_require_a_session_and_logout_clears_the_cookie() {
        let request = Request::builder()
            .uri("/api/session")
            .header(header::HOST, "localhost:3000")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            test_app().oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/logout")
            .header(header::HOST, "localhost:3000")
            .header(header::ORIGIN, "http://localhost:3000")
            .body(Body::empty())
            .unwrap();
        let response = test_app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(
            response.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
    }
}
