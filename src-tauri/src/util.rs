use windows::core::PWSTR;
use windows::Win32::System::Com::CoTaskMemFree;

pub fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Leest een door COM gealloceerde string en geeft het geheugen vrij.
pub unsafe fn take_co_string(p: PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = p.to_string().ok();
    CoTaskMemFree(Some(p.0 as *const _));
    s
}

/// Schrijft een regel naar %APPDATA%\Dockey\dockey.log (handig om problemen
/// op te sporen). Het bestand begint opnieuw zodra het groter dan 256 kB wordt.
pub fn log(msg: &str) {
    use std::io::Write;
    let dir = crate::config::data_dir();
    let path = dir.join("dockey.log");
    if std::fs::metadata(&path).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    let _ = std::fs::create_dir_all(&dir);
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{secs}] {msg}");
    }
}
