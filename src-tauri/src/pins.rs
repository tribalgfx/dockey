//! Eenmalig de apps overnemen die al aan de Windows-taakbalk zijn vastgemaakt.

use crate::config::Pinned;
use crate::util::from_wide;
use std::path::PathBuf;
use windows::core::{Interface, GUID, HSTRING};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

/// System.AppUserModel.ID
const PKEY_APPUSERMODEL_ID: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3), pid: 5 };

fn pin_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\TaskBar"))
}

/// Doel (exe-pad) en eventuele AppUserModelID van een snelkoppeling.
unsafe fn resolve(lnk: &str) -> windows::core::Result<(String, String)> {
    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
    link.cast::<IPersistFile>()?.Load(&HSTRING::from(lnk), STGM_READ)?;

    let mut buf = [0u16; 1024];
    let _ = link.GetPath(&mut buf, std::ptr::null_mut(), 0);
    let target = from_wide(&buf);

    let aumid = link
        .cast::<IPropertyStore>()
        .and_then(|store| store.GetValue(&PKEY_APPUSERMODEL_ID))
        .map(|v| v.to_string())
        .unwrap_or_default();
    Ok((target, aumid))
}

/// Moet op een thread met COM draaien.
pub fn import_windows_pins() -> Vec<Pinned> {
    let Some(dir) = pin_dir() else { return vec![] };
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case("lnk")).unwrap_or(false))
        .collect();
    files.sort();

    let mut out: Vec<Pinned> = Vec::new();
    for file in files {
        let lnk = file.to_string_lossy().to_string();
        let name = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let (target, aumid) = unsafe { resolve(&lnk) }.unwrap_or_default();

        let key = if aumid.eq_ignore_ascii_case("Microsoft.Windows.Explorer") {
            // Verkenner heeft geen exe als doel; vensters draaien onder explorer.exe.
            let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
            format!("{windir}\\explorer.exe").to_lowercase()
        } else if target.to_lowercase().ends_with(".exe") {
            target.to_lowercase()
        } else if !aumid.is_empty() && aumid.contains('!') {
            format!("aumid:{aumid}")
        } else {
            lnk.to_lowercase()
        };
        if !out.iter().any(|p| p.key == key) {
            out.push(Pinned { key, name, launch: lnk });
        }
    }
    out
}
