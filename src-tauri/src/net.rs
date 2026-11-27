//! Gegevens ophalen voor eigen widgets. Dit gebeurt hier en niet in de
//! webview: zo speelt CORS geen rol en kan een zelfondertekend certificaat op
//! het thuisnetwerk bewust worden geaccepteerd.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

// Een agenda met jaren aan afspraken kan een paar MB zijn.
const MAX_BYTES: u64 = 8 * 1024 * 1024;

// Foutcodes die de interface in de gekozen taal toont (zie errorText in
// ui/i18n.js); andere fouten komen als technische tekst door.
fn get(url: &str, insecure: bool) -> Result<ureq::Response, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("ERR_SCHEME".into());
    }
    let tls = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(insecure)
        .danger_accept_invalid_hostnames(insecure)
        .build()
        .map_err(|e| e.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .timeout(Duration::from_secs(20))
        .user_agent("Dockey")
        .build();
    agent.get(url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("ERR_HTTP:{code}"),
        other => other.to_string(),
    })
}

/// Het hele antwoord, of `ERR_TOO_LARGE` als het groter is dan MAX_BYTES (een
/// afgekapt antwoord zou een halve agenda of onleesbare JSON opleveren).
fn read_body(response: ureq::Response) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    response.into_reader().take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("ERR_TOO_LARGE".into());
    }
    Ok(bytes)
}

pub fn fetch(url: &str, insecure: bool) -> Result<String, String> {
    let bytes = read_body(get(url, insecure)?)?;
    // Niet-UTF-8 (bijv. een oude Latin-1-feed) niet weigeren: hooguit een
    // enkel teken wordt dan een vervangingsteken.
    Ok(String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

/// Een afbeelding ophalen als data-URL (bijv. het icoon van een statuswidget).
pub fn fetch_image(url: &str, insecure: bool) -> Result<String, String> {
    use base64::Engine;
    let response = get(url, insecure)?;
    let declared = response.content_type().to_string();
    let bytes = read_body(response)?;
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if declared.starts_with("image/") {
        declared.as_str()
    } else {
        return Err("ERR_NOT_IMAGE".into());
    };
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)))
}
