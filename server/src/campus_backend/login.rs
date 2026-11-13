use anyhow::{Context, Result, ensure};
use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, Url};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use scraper::{Html, Selector};

use crate::{
    campus_backend::odata::fetch_user_info,
    constants::CD_CERT_PEM,
    types::{CampusLoginData, CdAuthData, UserBasicInfo},
};

const PORTAL_URL: &str = "https://fep.campus-dual.de/portal";
const IDP_LOGIN_URL: &str = "https://idp.dhsn.de/login";
const DEFAULT_ACS_URL: &str = "https://fep.campus-dual.de/sap/saml2/sp/acs/100";
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/117.0.0.0 Safari/537.36";

pub async fn cdlogin_get_jcookie_and_meta(
    login_data: CampusLoginData,
) -> Result<(CdAuthData, UserBasicInfo)> {
    let cookie_store = Arc::new(CookieStoreMutex::new(CookieStore::new()));

    let client = reqwest::Client::builder()
        .add_root_certificate(CD_CERT_PEM.get().unwrap().clone())
        .cookie_provider(cookie_store.clone())
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("too many Campus Dual redirects");
            }
            if is_trusted_redirect(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("untrusted Campus Dual redirect")
            }
        }))
        .build()?;

    campus_login(&client, &login_data).await?;

    let user_basic_info = fetch_user_info(&client, &login_data.username).await?;

    let cd_auth_data = CdAuthData {
        cookie: extract_cd_cookies(cookie_store)?,
        user: login_data.username,
        password: login_data.password,
    };

    Ok((cd_auth_data, user_basic_info))
}

fn is_trusted_redirect(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && matches!(url.host_str(), Some("fep.campus-dual.de" | "idp.dhsn.de"))
        && url.username().is_empty()
        && url.password().is_none()
}

fn trusted_portal_action(action: &str) -> Result<Url> {
    let url = Url::parse(PORTAL_URL)?.join(action)?;
    ensure!(
        is_trusted_redirect(&url) && url.host_str() == Some("fep.campus-dual.de"),
        "untrusted SAML form action"
    );
    Ok(url)
}

/// SAML web SSO: portal -> IdP login form -> SAMLResponse -> ACS of the SAP system.
async fn campus_login(client: &Client, login_data: &CampusLoginData) -> Result<()> {
    // The portal redirects to the IdP login page, which carries the CSRF token.
    let login_page = client
        .get(PORTAL_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let csrf = {
        let document = Html::parse_document(&login_page);
        document
            .select(&Selector::parse(r#"input[name="_token"]"#).unwrap())
            .next()
            .context("CD login stage 1: XSRF token missing")?
            .value()
            .attr("value")
            .context("CD login stage 1: XSRF token has no value")?
            .to_string()
    };

    let form = [
        ("_token", csrf.as_str()),
        ("username", login_data.username.as_str()),
        ("password", login_data.password.as_str()),
        ("remember", "0"),
    ];

    // Redirects (release-attributes -> saml/login) are followed automatically.
    let saml_page = client
        .post(IDP_LOGIN_URL)
        .form(&form)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let (acs_url, saml_response, relay_state) = {
        let document = Html::parse_document(&saml_page);
        let saml_response = document
            .select(&Selector::parse(r#"input[name="SAMLResponse"]"#).unwrap())
            .next()
            .and_then(|e| e.value().attr("value"))
            // no assertion means the IdP rejected the credentials
            .context("CD login stage 2: SAMLResponse missing")?
            .to_string();
        let relay_state = document
            .select(&Selector::parse(r#"input[name="RelayState"]"#).unwrap())
            .next()
            .and_then(|e| e.value().attr("value"))
            .unwrap_or_default()
            .to_string();
        let acs_action = document
            .select(&Selector::parse("form[action]").unwrap())
            .filter_map(|f| f.value().attr("action"))
            .find(|action| trusted_portal_action(action).is_ok())
            .unwrap_or(DEFAULT_ACS_URL);
        let acs_url = trusted_portal_action(acs_action)?;
        (acs_url, saml_response, relay_state)
    };

    // The ACS sets the SAP session cookies and redirects to the portal.
    let acs_resp = client
        .post(acs_url)
        .form(&[
            ("SAMLResponse", saml_response.as_str()),
            ("RelayState", relay_state.as_str()),
        ])
        .send()
        .await?
        .error_for_status()?;

    for (k, v) in acs_resp.headers() {
        if k.as_str().eq_ignore_ascii_case("set-cookie") {
            log::debug!(
                "ACS set-cookie: {:?}",
                v.to_str().map(|s| s.split('=').next().unwrap_or(""))
            );
        }
    }

    let acs_text = acs_resp.text().await?;
    log::info!("ACS response length: {}", acs_text.len());

    // In SAP SAML2 SSO, ACS returns an auto-submitting form targeting /portal
    let follow_up: Option<(Url, Vec<(String, String)>)> = {
        let acs_document = Html::parse_document(&acs_text);
        if let Some(form_elem) = acs_document
            .select(&Selector::parse("form").unwrap())
            .next()
        {
            let action = form_elem.value().attr("action").unwrap_or("/portal");
            let target_url = trusted_portal_action(action)?;

            let mut form_fields = Vec::new();
            for input in form_elem.select(&Selector::parse("input").unwrap()) {
                if let Some(name) = input.value().attr("name") {
                    let val = input.value().attr("value").unwrap_or_default();
                    form_fields.push((name.to_string(), val.to_string()));
                }
            }
            if form_fields.is_empty() {
                form_fields.push(("SAMLResponse".to_string(), saml_response.clone()));
                form_fields.push(("RelayState".to_string(), relay_state.clone()));
            }
            Some((target_url, form_fields))
        } else {
            None
        }
    };

    if let Some((target_url, form_fields)) = follow_up {
        log::info!("Submitting the Campus Dual follow-up form");
        let follow_resp = client
            .post(target_url)
            .form(&form_fields)
            .send()
            .await?
            .error_for_status()?;
        log::info!("Follow-up form response: {}", follow_resp.status());
        for (k, v) in follow_resp.headers() {
            if k.as_str().eq_ignore_ascii_case("set-cookie") {
                log::debug!(
                    "Follow-up set-cookie: {:?}",
                    v.to_str().map(|s| s.split('=').next().unwrap_or(""))
                );
            }
        }
    } else {
        // Fallback: POST directly to /portal with SAMLResponse & RelayState (HAR entry 25)
        log::info!("No form in ACS response, posting to /portal fallback");
        let fallback_resp = client
            .post(PORTAL_URL)
            .form(&[
                ("SAMLResponse", saml_response.as_str()),
                ("RelayState", relay_state.as_str()),
            ])
            .send()
            .await?
            .error_for_status()?;
        for (k, v) in fallback_resp.headers() {
            if k.as_str().eq_ignore_ascii_case("set-cookie") {
                log::debug!(
                    "Fallback set-cookie: {:?}",
                    v.to_str().map(|s| s.split('=').next().unwrap_or(""))
                );
            }
        }
    }

    // Ensure session is active on portal (HAR entry 26)
    let portal_resp = client.get(PORTAL_URL).send().await?.error_for_status()?;
    for (k, v) in portal_resp.headers() {
        if k.as_str().eq_ignore_ascii_case("set-cookie") {
            log::debug!(
                "Portal GET set-cookie: {:?}",
                v.to_str().map(|s| s.split('=').next().unwrap_or(""))
            );
        }
    }

    log::info!("campus_login success");

    Ok(())
}

const ODATA_COOKIE_URL: &str = "https://fep.campus-dual.de/sap/opu/odata/sap/";

/// Persists only the cookies the browser would send to the OData endpoints.
fn extract_cd_cookies(cookie_store: Arc<CookieStoreMutex>) -> Result<String> {
    let store = cookie_store.lock().unwrap();
    let url = Url::parse(ODATA_COOKIE_URL)?;
    let pairs: Vec<(String, String)> = store
        .get_request_values(&url)
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect();

    log::info!(
        "Session cookies for fep: {:?}",
        pairs.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
    );

    if !pairs.iter().any(|(n, _)| n.starts_with("SAP_SESSIONID")) {
        anyhow::bail!("c-d.de cookie missing");
    }

    Ok(serde_json::to_string(&pairs)?)
}

pub fn cookie_store_from_json(j_cookies: &str) -> Result<Arc<CookieStoreMutex>> {
    let pairs: Vec<(String, String)> = serde_json::from_str(j_cookies)?;
    let url = Url::parse(ODATA_COOKIE_URL)?;
    let cookie_store = Arc::new(CookieStoreMutex::new(CookieStore::new()));
    {
        let mut store = cookie_store.lock().unwrap();
        for (name, value) in pairs {
            store.parse(&format!("{name}={value}; Path=/"), &url)?;
        }
    }
    Ok(cookie_store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saml_actions_are_restricted_to_the_fep_host() {
        assert!(trusted_portal_action("/portal").is_ok());
        assert!(trusted_portal_action("https://fep.campus-dual.de/portal").is_ok());
        assert!(trusted_portal_action("https://example.com/?fep.campus-dual.de").is_err());
        assert!(trusted_portal_action("http://fep.campus-dual.de/portal").is_err());
        assert!(trusted_portal_action("https://fep.campus-dual.de.example.com/").is_err());
    }

    #[test]
    fn redirects_only_allow_expected_https_hosts() {
        assert!(is_trusted_redirect(
            &Url::parse("https://idp.dhsn.de/login").unwrap()
        ));
        assert!(!is_trusted_redirect(
            &Url::parse("https://idp.dhsn.de.example.com/").unwrap()
        ));
    }
}
