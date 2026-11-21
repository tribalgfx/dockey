//! Systeemgegevens voor de widgets: CPU, geheugen, accu, netwerk en volume.

use serde::Serialize;
use std::time::Instant;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// 0–100
    pub cpu: f32,
    /// 0–100
    pub ram: f32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    /// None op een pc zonder accu.
    pub battery: Option<u8>,
    pub charging: bool,
    /// bytes per seconde
    pub down: f64,
    pub up: f64,
    /// 0–1; None als er geen audioapparaat is.
    pub volume: Option<f32>,
    pub muted: bool,
}

fn ft(t: FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
}

fn cpu_times() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    // "kernel" bevat ook de idle-tijd.
    Some((ft(idle), ft(kernel) + ft(user)))
}

/// Totaal ontvangen/verzonden bytes over alle echte (hardware) adapters.
fn net_totals() -> Option<(u64, u64)> {
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table).0 != 0 || table.is_null() {
            return None;
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let (mut rx, mut tx) = (0u64, 0u64);
        for row in rows {
            // Bit 0 = HardwareInterface, bit 1 = FilterInterface.
            let flags = row.InterfaceAndOperStatusFlags._bitfield;
            if flags & 0b01 != 0 && flags & 0b10 == 0 && row.OperStatus == IfOperStatusUp {
                rx += row.InOctets;
                tx += row.OutOctets;
            }
        }
        FreeMibTable(table as *const _);
        Some((rx, tx))
    }
}

fn endpoint_volume() -> windows::core::Result<IAudioEndpointVolume> {
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
    }
}

pub fn set_volume(level: f32) -> windows::core::Result<()> {
    unsafe { endpoint_volume()?.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null()) }
}

pub fn set_mute(muted: bool) -> windows::core::Result<()> {
    unsafe { endpoint_volume()?.SetMute(muted, std::ptr::null()) }
}

pub struct Sampler {
    cpu: Option<(u64, u64)>,
    net: Option<(u64, u64, Instant)>,
}

impl Sampler {
    pub fn new() -> Self {
        Self { cpu: cpu_times(), net: net_totals().map(|(r, t)| (r, t, Instant::now())) }
    }

    pub fn sample(&mut self) -> Stats {
        let mut s = Stats::default();

        if let Some((idle, total)) = cpu_times() {
            if let Some((idle0, total0)) = self.cpu {
                let dt = total.saturating_sub(total0);
                if dt > 0 {
                    let busy = dt.saturating_sub(idle.saturating_sub(idle0));
                    s.cpu = (busy as f32 / dt as f32 * 100.0).clamp(0.0, 100.0);
                }
            }
            self.cpu = Some((idle, total));
        }

        let mut mem = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        if unsafe { GlobalMemoryStatusEx(&mut mem) }.is_ok() {
            const GB: f32 = 1024.0 * 1024.0 * 1024.0;
            s.ram = mem.dwMemoryLoad as f32;
            s.ram_total_gb = mem.ullTotalPhys as f32 / GB;
            s.ram_used_gb = mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys) as f32 / GB;
        }

        let mut power = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut power) }.is_ok() {
            // BatteryFlag 128 = geen accu; 255 = onbekend.
            if power.BatteryFlag & 128 == 0 && power.BatteryFlag != 255 && power.BatteryLifePercent <= 100 {
                s.battery = Some(power.BatteryLifePercent);
                s.charging = power.ACLineStatus == 1;
            }
        }

        if let Some((rx, tx)) = net_totals() {
            let now = Instant::now();
            if let Some((rx0, tx0, t0)) = self.net {
                let secs = now.duration_since(t0).as_secs_f64();
                if secs > 0.0 {
                    s.down = rx.saturating_sub(rx0) as f64 / secs;
                    s.up = tx.saturating_sub(tx0) as f64 / secs;
                }
            }
            self.net = Some((rx, tx, now));
        }

        if let Ok(vol) = endpoint_volume() {
            unsafe {
                s.volume = vol.GetMasterVolumeLevelScalar().ok();
                s.muted = vol.GetMute().map(|m| m.as_bool()).unwrap_or(false);
            }
        }
        s
    }
}
