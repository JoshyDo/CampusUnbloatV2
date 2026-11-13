use std::{env, path::PathBuf, time::Duration};

use anyhow::{Context, Result, ensure};
use tokio::net::TcpListener;

mod auth;
mod campus_backend;
mod color_stuff;
mod constants;
mod error;
mod remember;
mod routes;
mod services;
mod types;

#[tokio::main]
async fn main() -> Result<()> {
    if env::var(pretty_env_logger::env_logger::DEFAULT_FILTER_ENV).is_err() {
        pretty_env_logger::formatted_timed_builder()
            .filter_level(log::LevelFilter::Info)
            .init();
    } else {
        pretty_env_logger::init_timed();
    }

    let port: u16 = env::var("CAMPUSUNBLOAT_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .context("CAMPUSUNBLOAT_PORT must be a valid port number")?;
    ensure!(port != 0, "CAMPUSUNBLOAT_PORT must not be zero");
    let web_dir = env::var_os("CAMPUSUNBLOAT_WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("web").join("dist"));
    ensure!(
        web_dir.join("index.html").is_file(),
        "Web application not built. Run `trunk build --release` in the web directory first."
    );

    let cert = reqwest::Certificate::from_pem(include_bytes!("GEANT TLS RSA 1.pem"))?;
    constants::CD_CERT_PEM
        .set(cert)
        .map_err(|_| anyhow::anyhow!("Campus Dual certificate was already initialized"))?;

    let content_security_policy = routes::content_security_policy(&web_dir)?;
    let state = auth::AppState::default();
    let cleanup = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            cleanup.remove_expired().await;
        }
    });

    let mensa = mensa_api::app()
        .await
        .context("Mensa cache initialization failed")?;
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .context("Could not start the local web server")?;
    log::info!("CampusUnbloatV2: http://localhost:{port}");
    axum::serve(
        listener,
        routes::app(state, mensa, web_dir, port, content_security_policy),
    )
    .with_graceful_shutdown(async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            log::error!("Could not install shutdown handler: {error}");
        }
    })
    .await
    .context("The local web server stopped unexpectedly")?;
    Ok(())
}
