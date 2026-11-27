//! Updates: Dockey vraagt bij GitHub de nieuwste release op en biedt die aan
//! als die nieuwer is. Pas na een klik op "Nu bijwerken" downloadt Dockey de
//! installer, controleert hem met het SHA-256-controlegetal dat GitHub bij de
//! release vermeldt én met de handtekening ernaast (`<installer>.sig`), en
//! start hem stil. De installer sluit Dockey netjes af, vervangt het programma
//! en start het daarna weer (/STARTAFTER=1, zie installer\Dockey.iss).
//!
//! De handtekening (ECDSA P-256 over SHA-256) is gezet met een privésleutel die
//! niet op GitHub staat (zie scripts\sign-installer.ps1); de publieke sleutel
//! zit in Dockey zelf (update-key.pub). Wie toegang tot GitHub krijgt, kan dus
//! nog geen installer maken die Dockey accepteert. Een release zonder (geldige)
//! handtekening bieden we niet aan.
//!
//! Bij het kijken gaat er niets over de gebruiker mee: alleen een verzoek om de
//! nieuwste release, met "Dockey" als User-Agent.

use serde::Serialize;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

const LATEST: &str = "https://api.github.com/repos/tribalgfx/dockey/releases/latest";
/// De releases van Dockey zelf: alleen daarvandaan downloaden we, en alleen
/// daarheen gaat de knop naar de releasepagina.
const RELEASES: &str = "https://github.com/tribalgfx/dockey/releases";
/// Publieke sleutel voor de handtekening: X en Y van een P-256-punt, in base64.
const PUBLIC_KEY: &str = include_str!("../update-key.pub");
/// Ruim boven de echte grootte (ongeveer 3 MB): alleen een vangnet.
const MAX_INSTALLER_BYTES: u64 = 200 * 1024 * 1024;
/// Een handtekening is minder dan honderd tekens.
const MAX_SIGNATURE_BYTES: u64 = 4096;

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
    #[serde(skip)]
    signature_url: String,
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

/// Een downloadadres bij een release van Dockey zelf?
fn own_download(url: &str) -> bool {
    url.strip_prefix(RELEASES).is_some_and(|rest| rest.starts_with("/download/"))
}

/// Leest het antwoord van /releases/latest. Concepten, voorlopige versies,
/// onleesbare tags en releases zonder controleerbare, ondertekende installer
/// bieden we niet aan.
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
    let asset = |wanted: &str| v["assets"].as_array().and_then(|list| list.iter().find(|a| a["name"].as_str() == Some(wanted)));
    let (Some(installer), Some(signature)) = (asset(&name), asset(&format!("{name}.sig"))) else {
        return Ok(None);
    };
    let (Some(url), Some(size), Some(sha256), Some(signature_url)) = (
        installer["browser_download_url"].as_str().filter(|u| own_download(u)),
        installer["size"].as_u64(),
        installer["digest"].as_str().and_then(sha256_from),
        signature["browser_download_url"].as_str().filter(|u| own_download(u)),
    ) else {
        return Ok(None);
    };
    let page_prefix = format!("{RELEASES}/");
    Ok(Some(Release {
        version,
        notes: v["body"].as_str().unwrap_or("").chars().take(4000).collect(),
        // Alleen een pagina bij de releases van Dockey; anders de releaselijst.
        page: v["html_url"].as_str().filter(|p| p.starts_with(&page_prefix)).unwrap_or(RELEASES).to_string(),
        url: url.to_string(),
        name,
        size,
        sha256,
        signature_url: signature_url.to_string(),
    }))
}

/// "sha256:ABC…" → "abc…"; iets anders (ander algoritme, geen 64 tekens) geeft None.
fn sha256_from(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?.trim().to_ascii_lowercase();
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

/// Downloadt de installer, controleert grootte, SHA-256 en handtekening en start hem stil.
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
    let digest = sha256(&bytes)?;
    if hex(&digest) != release.sha256 {
        return Err("ERR_UPDATE_HASH".into());
    }
    let signature = download_signature(&release.signature_url)?;
    if !verify_signature(PUBLIC_KEY, &digest, &signature) {
        return Err("ERR_UPDATE_SIGNATURE".into());
    }

    let dir = std::env::temp_dir().join("Dockey-update");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(&release.name);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    // Vlak voor het starten nog eens: het bestand moet precies zijn wat we controleerden.
    let written = std::fs::read(&path).map_err(|e| e.to_string())?;
    if sha256(&written)? != digest {
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

/// De handtekening bij de installer: base64 van r en s (samen 64 bytes).
fn download_signature(url: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let mut text = String::new();
    agent()?
        .get(url)
        .call()
        .map_err(|e| e.to_string())?
        .into_reader()
        .take(MAX_SIGNATURE_BYTES)
        .read_to_string(&mut text)
        .map_err(|_| "ERR_UPDATE_SIGNATURE".to_string())?;
    base64::engine::general_purpose::STANDARD.decode(text.trim()).map_err(|_| "ERR_UPDATE_SIGNATURE".to_string())
}

/// Klopt `signature` (r en s, elk 32 bytes) bij deze SHA-256 en de publieke
/// sleutel (base64 van X en Y)? Met de cryptografie van Windows (CNG).
fn verify_signature(public_key: &str, digest: &[u8; 32], signature: &[u8]) -> bool {
    use base64::Engine;
    use windows::Win32::Security::Cryptography::{
        BCryptDestroyKey, BCryptImportKeyPair, BCryptVerifySignature, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALG_HANDLE,
        BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_FLAGS, BCRYPT_KEY_HANDLE,
    };
    let Ok(xy) = base64::engine::general_purpose::STANDARD.decode(public_key.trim()) else { return false };
    if xy.len() != 64 || signature.len() != 64 {
        return false;
    }
    // BCRYPT_ECCKEY_BLOB: kenmerk, sleutellengte in bytes, dan X en Y.
    let mut blob = Vec::with_capacity(72);
    blob.extend_from_slice(&BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
    blob.extend_from_slice(&32u32.to_le_bytes());
    blob.extend_from_slice(&xy);
    unsafe {
        let mut key = BCRYPT_KEY_HANDLE::default();
        if BCryptImportKeyPair(BCRYPT_ECDSA_P256_ALG_HANDLE, None, BCRYPT_ECCPUBLIC_BLOB, &mut key, &blob, 0).is_err() {
            return false;
        }
        let ok = BCryptVerifySignature(key, None, digest, signature, BCRYPT_FLAGS(0)).is_ok();
        let _ = BCryptDestroyKey(key);
        ok
    }
}

/// SHA-256 met de cryptografie van Windows (CNG).
fn sha256(data: &[u8]) -> Result<[u8; 32], String> {
    use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut out = [0u8; 32];
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    if status.is_err() {
        return Err(format!("SHA-256 berekenen mislukt ({:#x})", status.0));
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DL: &str = "https://github.com/tribalgfx/dockey/releases/download";

    fn release_with(tag: &str, draft: bool, digest: Option<&str>, base: &str, signed: bool) -> String {
        let version = tag.trim_start_matches('v');
        let digest = digest.map(|d| format!(r#","digest":"{d}""#)).unwrap_or_default();
        let sig = if signed {
            format!(r#",{{"name":"Dockey-Setup-{version}.exe.sig","size":88,"browser_download_url":"{base}/{tag}/Dockey-Setup-{version}.exe.sig"}}"#)
        } else {
            String::new()
        };
        format!(
            r#"{{"tag_name":"{tag}","draft":{draft},"prerelease":false,"body":"Nieuw","html_url":"https://github.com/tribalgfx/dockey/releases/tag/{tag}",
               "assets":[{{"name":"Dockey-Setup-{version}.exe","size":3,"browser_download_url":"{base}/{tag}/Dockey-Setup-{version}.exe"{digest}}}{sig}]}}"#
        )
    }

    fn release_json(tag: &str, draft: bool, digest: Option<&str>) -> String {
        release_with(tag, draft, digest, DL, true)
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
        assert_eq!(r.signature_url, format!("{DL}/v99.0.0/Dockey-Setup-99.0.0.exe.sig"));
        assert_eq!(r.page, "https://github.com/tribalgfx/dockey/releases/tag/v99.0.0");
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
    fn alleen_eigen_ondertekende_downloads() {
        // Zonder handtekening, of een download van ergens anders: niet aanbieden.
        assert!(parse(&release_with("v99.0.0", false, Some(DIGEST), DL, false)).unwrap().is_none());
        assert!(parse(&release_with("v99.0.0", false, Some(DIGEST), "https://example.org/x", true)).unwrap().is_none());
        let lookalike = "https://github.com/tribalgfx/dockey/releasesx/download";
        assert!(parse(&release_with("v99.0.0", false, Some(DIGEST), lookalike, true)).unwrap().is_none());
        // Een vreemde releasepagina wordt de releaselijst.
        let body = release_json("v99.0.0", false, Some(DIGEST))
            .replace("https://github.com/tribalgfx/dockey/releases/tag/v99.0.0", "file:///C:/x.exe");
        assert_eq!(parse(&body).unwrap().expect("update").page, RELEASES);
    }

    // Een testsleutel, alleen voor deze test gemaakt met scripts\sign-installer.ps1,
    // en zijn handtekening over "abc".
    const TEST_KEY: &str = "YO8ehODzPzAakpi2jDLvWImOoEYr62/8pmAyyABgKLUJWaAzX36UQlNehlfq1wCvOPCdk9blKsTNtB6mfRXodw==";
    const TEST_SIG: &str = "iX3s/x1F5Mbr3EtPzxoTseX4gypWGh66pVtov61Z9Lid7UXP4g2Z1TNFtvJvHM9T5VbFwdngZtQesJJhmbgEHg==";

    #[test]
    fn handtekening() {
        use base64::Engine;
        let sig = base64::engine::general_purpose::STANDARD.decode(TEST_SIG).unwrap();
        let abc = sha256(b"abc").unwrap();
        assert!(verify_signature(TEST_KEY, &abc, &sig));
        // Ander bestand, aangepaste handtekening of andere sleutel: geweigerd.
        assert!(!verify_signature(TEST_KEY, &sha256(b"abd").unwrap(), &sig));
        let mut bad = sig.clone();
        bad[10] ^= 1;
        assert!(!verify_signature(TEST_KEY, &abc, &bad));
        assert!(!verify_signature(PUBLIC_KEY, &abc, &sig));
        assert!(!verify_signature("geen sleutel", &abc, &sig));
    }

    #[test]
    fn publieke_sleutel_is_geldig() {
        use base64::Engine;
        let xy = base64::engine::general_purpose::STANDARD.decode(PUBLIC_KEY.trim()).expect("base64");
        assert_eq!(xy.len(), 64);
    }

    #[test]
    fn sha256_van_abc() {
        assert_eq!(hex(&sha256(b"abc").unwrap()), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
