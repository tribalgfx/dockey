//! Live venstervoorbeelden boven een app-icoon, zoals de Windows-taakbalk.
//!
//! Het voorbeeldvenster ("preview") is een eigen, focusloos venster: hoveren
//! mag de focus niet weghalen bij het venster waar je mee bezig bent. De
//! interface daarin tekent per venster de titel, het icoon en de sluitknop en
//! meldt waar de beelden moeten komen. Die beelden zelf zijn DWM-thumbnails:
//! Windows tekent daar een live kopie van het bronvenster bovenop ons venster.
//!
//! Verloop: de balk vraagt `open` aan; wij koppelen de thumbnails (nog
//! onzichtbaar) en geven de verhouding van elk venster door aan de interface.
//! Die maakt de indeling, meet en meldt zich met de rechthoeken (`ready`),
//! waarna wij het venster plaatsen, de thumbnails op hun plek zetten en het
//! venster tonen zonder het te activeren.

use std::collections::HashMap;
use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmUnregisterThumbnail,
    DwmUpdateThumbnailProperties, DWMWA_EXTENDED_FRAME_BOUNDS, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY,
    DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsIconic, IsWindow};

struct Thumb {
    id: isize,
    /// Het zichtbare deel van het bronvenster, in de coördinaten van de
    /// thumbnail: zonder de onzichtbare sleeprand die Windows 11 om vensters legt.
    source: RECT,
}

#[derive(Default)]
pub struct Preview {
    pub visible: bool,
    /// Groep waarvan het voorbeeld openstaat of net werd aangevraagd.
    pub key: String,
    /// Midden van het icoon, in schermpixels.
    pub anchor: (i32, i32),
    /// Telt op bij elk openen en sluiten; een `ready` van een eerdere aanvraag
    /// (die al ingehaald is) wordt genegeerd.
    pub serial: u64,
    thumbs: HashMap<isize, Thumb>,
}

/// Plek van een thumbnail binnen het voorbeeldvenster, in fysieke pixels.
pub struct Slot {
    pub hwnd: isize,
    pub rect: RECT,
}

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

impl Preview {
    /// Koppelt thumbnails voor deze vensters (al gekoppelde blijven staan,
    /// overbodige gaan weg). Geeft per venster de verhouding breedte/hoogte
    /// van het zichtbare deel terug, of `None` als er geen beeld is (bijv. een
    /// geminimaliseerd venster: dan toont de interface icoon en titel).
    pub fn sync(&mut self, dest: HWND, sources: &[isize]) -> HashMap<isize, f64> {
        self.thumbs.retain(|h, t| {
            let keep = sources.contains(h) && unsafe { !IsIconic(hwnd(*h)).as_bool() };
            if !keep {
                unsafe {
                    let _ = DwmUnregisterThumbnail(t.id);
                }
            }
            keep
        });
        let mut ratios = HashMap::new();
        for &raw in sources {
            let src = hwnd(raw);
            if unsafe { !IsWindow(Some(src)).as_bool() || IsIconic(src).as_bool() } {
                continue;
            }
            if !self.thumbs.contains_key(&raw) {
                match unsafe { DwmRegisterThumbnail(dest, src) } {
                    Ok(id) => {
                        self.thumbs.insert(raw, Thumb { id, source: RECT::default() });
                    }
                    Err(_) => continue,
                }
            }
            let thumb = self.thumbs.get_mut(&raw).unwrap();
            let Some(source) = visible_part(thumb.id, src) else { continue };
            thumb.source = source;
            let (w, h) = (source.right - source.left, source.bottom - source.top);
            if w > 0 && h > 0 {
                ratios.insert(raw, w as f64 / h as f64);
            }
        }
        ratios
    }

    /// Zet de thumbnails op hun plek; vensters zonder plek gaan onzichtbaar.
    pub fn place(&self, slots: &[Slot]) {
        for (raw, thumb) in &self.thumbs {
            let slot = slots.iter().find(|s| s.hwnd == *raw);
            let props = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_VISIBLE
                    | DWM_TNP_RECTDESTINATION
                    | DWM_TNP_RECTSOURCE
                    | DWM_TNP_OPACITY
                    | DWM_TNP_SOURCECLIENTAREAONLY,
                rcDestination: slot.map(|s| s.rect).unwrap_or_default(),
                rcSource: thumb.source,
                opacity: 255,
                fVisible: slot.is_some().into(),
                fSourceClientAreaOnly: false.into(),
            };
            unsafe {
                let _ = DwmUpdateThumbnailProperties(thumb.id, &props);
            }
        }
    }

    /// Alle thumbnails los.
    pub fn clear(&mut self) {
        for (_, t) in self.thumbs.drain() {
            unsafe {
                let _ = DwmUnregisterThumbnail(t.id);
            }
        }
    }
}

/// Het zichtbare deel van het bronvenster binnen zijn thumbnail. Het
/// vensterkader (GetWindowRect) bevat bij Windows 11 een onzichtbare sleeprand
/// van een paar pixels; de echte rand staat in DWMWA_EXTENDED_FRAME_BOUNDS.
/// De bron kan in een andere schaal werken dan wij (DPI-virtualisatie), dus
/// rekenen we om naar de maat die DWM voor de thumbnail opgeeft.
fn visible_part(thumb: isize, src: HWND) -> Option<RECT> {
    let size = unsafe { DwmQueryThumbnailSourceSize(thumb) }.ok()?;
    let mut window = RECT::default();
    let mut frame = RECT::default();
    unsafe {
        GetWindowRect(src, &mut window).ok()?;
        let ok = DwmGetWindowAttribute(
            src,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame as *mut RECT as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        );
        if ok.is_err() {
            frame = window;
        }
    }
    let (ww, wh) = (window.right - window.left, window.bottom - window.top);
    if ww <= 0 || wh <= 0 || size.cx <= 0 || size.cy <= 0 {
        return None;
    }
    let (sx, sy) = (size.cx as f64 / ww as f64, size.cy as f64 / wh as f64);
    let clamp = |v: f64, max: i32| (v.round() as i32).clamp(0, max);
    let rect = RECT {
        left: clamp((frame.left - window.left) as f64 * sx, size.cx),
        top: clamp((frame.top - window.top) as f64 * sy, size.cy),
        right: clamp((frame.right - window.left) as f64 * sx, size.cx),
        bottom: clamp((frame.bottom - window.top) as f64 * sy, size.cy),
    };
    (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
}
