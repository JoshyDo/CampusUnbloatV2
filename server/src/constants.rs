use std::sync::OnceLock;

pub static CD_CERT_PEM: OnceLock<reqwest::Certificate> = OnceLock::new();
