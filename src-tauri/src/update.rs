//! Updates, zoals in DonkiePane: Dockey vraagt bij GitHub de nieuwste release
//! op en biedt die aan als die nieuwer is. Pas na een klik op "Nu bijwerken"
//! downloadt Dockey de installer, controleert hem met het SHA-256-controlegetal
//! dat GitHub bij de release vermeldt, en start hem stil. De installer sluit
//! Dockey netjes af, vervangt het programma en start het daarna weer
//! (/STARTAFTER=1, zie installer\Dockey.iss).
//!
//! Bij het kijken gaat er niets over de gebruiker mee: alleen een verzoek om de
//! nieuwste release, met "Dockey" als User-Agent.

use serde::Serialize;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

const LATEST: &str = "https://api.github.com/repos/tribalgfx/dockey/releases/latest";
/// Ruim boven de echte grootte (ongeveer 3 MB): alleen een vangnet.
const MAX_INSTALLER_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    /// Wat er nieuw is (de tekst van de release, Markdown).
    pub notes: String,
    /// De pagina van de release op GitHub.
    pub page: String,
    #[serde(skip)]
    url: String,
    #[serde(skip)]
    name: String,
    #[serde(skip)]
    size: u64,
    #[serde(skip)]
    sha256: String,
}

/// De versie van deze Dockey (uit Cargo.toml).
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// "v0.2.0" of "0.2" → (0, 2, 0); een achtervoegsel als "-beta" telt niet mee.
fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let core = text.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u32>());
    let major = parts.next()?.ok()?;
    let minor = parts.next().unwrap_or(Ok(0)).ok()?;
    let patch = parts.next().unwrap_or(Ok(0)).ok()?;
    Some((major, minor, patch))
}

fn agent() -> Result<ureq::Agent, String> {
    let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    Ok(ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .timeout(Duration::from_secs(60))
        .user_agent("Dockey")
        .build())
}

/// Is er een nieuwere release met een installer? `None` = deze Dockey is de nieuwste.
pub fn check() -> Result<Option<Release>, String> {
    let body = agent()?
        .get(LATEST)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    parse(&body)
}

/// Leest het antwoord van /releases/latest. Concepten, voorlopige versies,
/// onleesbare tags en releases zonder (controleerbare) installer bieden we niet aan.
fn parse(body: &str) -> Result<Option<Release>, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return Ok(None);
    }
    let Some(latest) = v["tag_name"].as_str().and_then(parse_version) else { return Ok(None) };
    if latest <= parse_version(current()).unwrap_or((0, 0, 0)) {
        return Ok(None);
    }
    let version = format!("{}.{}.{}", latest.0, latest.1, latest.2);
    let name = format!("Dockey-Setup-{version}.exe");
    let Some(asset) = v["assets"].as_array().and_then(|list| list.iter().find(|a| a["name"].as_str() == Some(name.as_str())))
    else {
        return Ok(None);
    };
    let (Some(url), Some(size), Some(sha256)) = (
        asset["browser_download_url"].as_str(),
        asset["size"].as_u64(),
        asset["digest"].as_str().and_then(sha256_from),
    ) else {
        return Ok(None);
    };
    Ok(Some(Release {
        version,
        notes: v["body"].as_str().unwrap_or("").chars().take(4000).collect(),
        page: v["html_url"].as_str().unwrap_or("").to_string(),
        url: url.to_string(),
        name,
        size,
        sha256,
    }))
}

/// "sha256:ABC…" → "abc…"; iets anders (ander algoritme, geen 64 tekens) geeft None.
fn sha256_from(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?.trim().to_ascii_lowercase();
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

/// Downloadt de installer, controleert grootte en SHA-256 en start hem stil.
/// Foutcodes (ERR_UPDATE_…) vertaalt de interface; andere fouten zijn technisch.
pub fn download_and_start(release: &Release) -> Result<(), String> {
    let response = agent()?.get(&release.url).call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(release.size.min(MAX_INSTALLER_BYTES) + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 != release.size {
        return Err("ERR_UPDATE_SIZE".into());
    }
    if sha256_hex(&bytes)? != release.sha256 {
        return Err("ERR_UPDATE_HASH".into());
    }

    let dir = std::env::temp_dir().join("Dockey-update");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(&release.name);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    // Vlak voor het starten nog eens: het bestand moet precies zijn wat we controleerden.
    let written = std::fs::read(&path).map_err(|e| e.to_string())?;
    if sha256_hex(&written)? != release.sha256 {
        let _ = std::fs::remove_file(&path);
        return Err("ERR_UPDATE_HASH".into());
    }
    std::process::Command::new(&path)
        .args(["/SILENT", "/NORESTART", "/STARTAFTER=1"])
        .spawn()
        .map_err(|e| e.to_string())?;
    crate::util::log(&format!("update naar {} gestart", release.version));
    Ok(())
}

/// SHA-256 met de cryptografie van Windows (CNG).
fn sha256_hex(data: &[u8]) -> Result<String, String> {
    use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut out = [0u8; 32];
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    if status.is_err() {
        return Err(format!("SHA-256 berekenen mislukt ({:#x})", status.0));
    }
    Ok(out.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_json(tag: &str, draft: bool, digest: Option<&str>) -> String {
        let version = tag.trim_start_matches('v');
        let digest = digest.map(|d| format!(r#","digest":"{d}""#)).unwrap_or_default();
        format!(
            r#"{{"tag_name":"{tag}","draft":{draft},"prerelease":false,"body":"Nieuw","html_url":"https://example.org/r",
               "assets":[{{"name":"Dockey-Setup-{version}.exe","size":3,"browser_download_url":"https://example.org/s.exe"{digest}}}]}}"#
        )
    }

    const DIGEST: &str = "sha256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";

    #[test]
    fn versies() {
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.3"), Some((1, 3, 0)));
        assert_eq!(parse_version("v2.0.1-beta"), Some((2, 0, 1)));
        assert_eq!(parse_version("nieuw"), None);
        assert!(parse_version("0.10.0") > parse_version("0.9.9"));
    }

    #[test]
    fn nieuwere_release_met_controlegetal() {
        let r = parse(&release_json("v99.0.0", false, Some(DIGEST))).unwrap().expect("update");
        assert_eq!(r.version, "99.0.0");
        assert_eq!(r.name, "Dockey-Setup-99.0.0.exe");
        assert_eq!(r.sha256, DIGEST[7..].to_ascii_lowercase());
    }

    #[test]
    fn niets_aanbieden() {
        // Dezelfde of een oudere versie.
        assert!(parse(&release_json(&format!("v{}", current()), false, Some(DIGEST))).unwrap().is_none());
        assert!(parse(&release_json("v0.0.1", false, Some(DIGEST))).unwrap().is_none());
        // Een concept, of een installer zonder controlegetal.
        assert!(parse(&release_json("v99.0.0", true, Some(DIGEST))).unwrap().is_none());
        assert!(parse(&release_json("v99.0.0", false, None)).unwrap().is_none());
        assert!(parse(&release_json("v99.0.0", false, Some("md5:abc"))).unwrap().is_none());
    }

    #[test]
    fn sha256_van_abc() {
        assert_eq!(sha256_hex(b"abc").unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}

