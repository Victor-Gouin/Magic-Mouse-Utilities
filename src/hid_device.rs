//! Énumération, connexion, activation Multi-Touch (`HidD_SetFeature`) et lecture
//! asynchrone (`FILE_FLAG_OVERLAPPED`) des périphériques HID Apple Magic Mouse sous Windows.

use crate::config::SharedConfig;
use crate::gestures::GestureEngine;
use crate::injector::EmulatedButton;
use crate::protocol::{parse_hid_report, MagicMouseModel, TouchPoint};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_GetAttributes, HidD_GetHidGuid, HidD_SetFeature, HIDD_ATTRIBUTES,
};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};
use windows::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};

#[derive(Debug, Clone)]
pub struct ConnectedDeviceInfo {
    pub model: MagicMouseModel,
    pub device_path: String,
    pub multitouch_unlocked: bool,
}

pub type SharedDeviceStatus = Arc<RwLock<Option<ConnectedDeviceInfo>>>;

/// État temps-réel de la surface capacitive et du moteur pour l'interface graphique (Radar Live).
#[derive(Debug, Clone)]
pub struct LiveTelemetry {
    pub touches: Vec<TouchPoint>,
    pub physical_click: bool,
    pub emulated_button: EmulatedButton,
    pub vel_x: f32,
    pub vel_y: f32,
    pub in_inertia: bool,
    pub packet_count: u64,
    pub last_packet_time: Option<Instant>,
}

impl Default for LiveTelemetry {
    fn default() -> Self {
        Self {
            touches: Vec::new(),
            physical_click: false,
            emulated_button: EmulatedButton::None,
            vel_x: 0.0,
            vel_y: 0.0,
            in_inertia: false,
            packet_count: 0,
            last_packet_time: None,
        }
    }
}

pub type SharedTelemetry = Arc<RwLock<LiveTelemetry>>;

/// Recherche toutes les interfaces HID Apple Magic Mouse connectées au système.
pub fn enumerate_magic_mice() -> Vec<(MagicMouseModel, Vec<u16>, String)> {
    let mut results = Vec::new();

    unsafe {
        let hid_guid = HidD_GetHidGuid();
        let dev_info = match SetupDiGetClassDevsW(
            Some(&hid_guid),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        ) {
            Ok(h) if !h.is_invalid() => h,
            _ => return results,
        };

        let mut member_index = 0u32;
        loop {
            let mut interface_data = SP_DEVICE_INTERFACE_DATA {
                cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };

            if SetupDiEnumDeviceInterfaces(
                dev_info,
                None,
                &hid_guid,
                member_index,
                &mut interface_data,
            )
            .is_err()
            {
                break;
            }
            member_index += 1;

            let mut required_size = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(
                dev_info,
                &interface_data,
                None,
                0,
                Some(&mut required_size),
                None,
            );

            if required_size == 0 {
                continue;
            }

            let mut buffer = vec![0u8; required_size as usize];
            let detail_ptr = buffer.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail_ptr).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;

            if SetupDiGetDeviceInterfaceDetailW(
                dev_info,
                &interface_data,
                Some(detail_ptr),
                required_size,
                None,
                None,
            )
            .is_err()
            {
                continue;
            }

            // Extraction du chemin UTF-16 terminé par NUL à partir de `DevicePath`
            let path_offset = std::mem::offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath);
            let u16_slice = std::slice::from_raw_parts(
                buffer[path_offset..].as_ptr() as *const u16,
                (required_size as usize - path_offset) / 2,
            );
            let nul_pos = u16_slice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(u16_slice.len());
            let mut path_w: Vec<u16> = u16_slice[..nul_pos].to_vec();
            path_w.push(0);

            let path_str = String::from_utf16_lossy(&path_w[..nul_pos]);

            // Ouverture sans droit de lecture/écriture strict (0) uniquement pour interroger VID/PID
            let handle = CreateFileW(
                PCWSTR(path_w.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                Default::default(),
                None,
            );

            if let Ok(h) = handle {
                if h != INVALID_HANDLE_VALUE {
                    let mut attrs = HIDD_ATTRIBUTES {
                        Size: size_of::<HIDD_ATTRIBUTES>() as u32,
                        ..Default::default()
                    };
                    if HidD_GetAttributes(h, &mut attrs).as_bool() {
                        if let Some(model) =
                            MagicMouseModel::from_vid_pid(attrs.VendorID, attrs.ProductID)
                        {
                            results.push((model, path_w, path_str));
                        }
                    }
                    let _ = CloseHandle(h);
                }
            }
        }

        let _ = SetupDiDestroyDeviceInfoList(dev_info);
    }

    results
}

/// Tente d'envoyer le Feature Report d'activation Multi-Touch sur un handle HID ouvert.
pub fn unlock_multitouch(handle: HANDLE, model: MagicMouseModel) -> bool {
    let report = model.multitouch_feature_report();
    let mut buf = report.to_vec();
    unsafe {
        HidD_SetFeature(
            handle,
            buf.as_mut_ptr() as *const c_void,
            buf.len() as u32,
        )
        .as_bool()
    }
}

/// Démarre le thread d'arrière-plan qui surveille la connexion de la Magic Mouse,
/// active le mode Multi-Touch et lit les trames capacitives en continu.
pub fn spawn_hid_worker(
    running: Arc<AtomicBool>,
    config: SharedConfig,
    status: SharedDeviceStatus,
    telemetry: SharedTelemetry,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut engine = GestureEngine::new();

        while running.load(Ordering::Relaxed) {
            let candidates = enumerate_magic_mice();
            let mut active_handle: Option<(HANDLE, MagicMouseModel, String, bool)> = None;

            for (model, path_w, path_str) in candidates {
                unsafe {
                    let h = CreateFileW(
                        PCWSTR(path_w.as_ptr()),
                        0x80000000u32 | 0x40000000u32,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        None,
                        OPEN_EXISTING,
                        FILE_FLAG_OVERLAPPED,
                        None,
                    );

                    let handle = match h {
                        Ok(valid) if valid != INVALID_HANDLE_VALUE => valid,
                        _ => match CreateFileW(
                            PCWSTR(path_w.as_ptr()),
                            0x80000000u32,
                            FILE_SHARE_READ | FILE_SHARE_WRITE,
                            None,
                            OPEN_EXISTING,
                            FILE_FLAG_OVERLAPPED,
                            None,
                        ) {
                            Ok(ro) if ro != INVALID_HANDLE_VALUE => ro,
                            _ => continue,
                        },
                    };

                    let unlocked = unlock_multitouch(handle, model);
                    if unlocked || active_handle.is_none() {
                        if let Some((old_h, _, _, _)) =
                            active_handle.replace((handle, model, path_str, unlocked))
                        {
                            let _ = CloseHandle(old_h);
                        }
                        if unlocked {
                            break;
                        }
                    } else {
                        let _ = CloseHandle(handle);
                    }
                }
            }

            let Some((handle, model, path_str, unlocked)) = active_handle else {
                if let Ok(mut st) = status.write() {
                    *st = None;
                }
                thread::sleep(Duration::from_millis(1500));
                continue;
            };

            if let Ok(mut st) = status.write() {
                *st = Some(ConnectedDeviceInfo {
                    model,
                    device_path: path_str,
                    multitouch_unlocked: unlocked,
                });
            }

            unsafe {
                let event = match CreateEventW(None, true, false, PCWSTR::null()) {
                    Ok(ev) => ev,
                    Err(_) => {
                        let _ = CloseHandle(handle);
                        thread::sleep(Duration::from_millis(1000));
                        continue;
                    }
                };

                let mut read_buf = [0u8; 256];
                let mut io_pending = false;
                let mut overlapped = OVERLAPPED::default();
                overlapped.hEvent = event;

                while running.load(Ordering::Relaxed) {
                    if !io_pending {
                        let _ = ResetEvent(event);
                        let mut bytes_read = 0u32;
                        let res = ReadFile(
                            handle,
                            Some(&mut read_buf),
                            Some(&mut bytes_read),
                            Some(&mut overlapped),
                        );

                        if res.is_ok() {
                            let n = bytes_read as usize;
                            if n > 0 {
                                if let Some(report) = parse_hid_report(model, &read_buf[..n]) {
                                    if let Ok(cfg) = config.read() {
                                        engine.process_report(&report, &cfg);
                                    }
                                    if let Ok(mut tel) = telemetry.write() {
                                        tel.touches = report
                                            .touches
                                            .iter()
                                            .copied()
                                            .filter(|t| t.is_down)
                                            .collect();
                                        tel.physical_click = report.physical_click;
                                        tel.emulated_button = engine.active_emulated_button();
                                        let (vx, vy) = engine.velocity();
                                        tel.vel_x = vx;
                                        tel.vel_y = vy;
                                        tel.in_inertia = engine.is_in_inertia();
                                        tel.packet_count += 1;
                                        tel.last_packet_time = Some(Instant::now());
                                    }
                                }
                            }
                            continue;
                        } else {
                            let err = GetLastError();
                            if err == ERROR_IO_PENDING {
                                io_pending = true;
                            } else {
                                break;
                            }
                        }
                    }

                    let wait_res = WaitForSingleObject(event, 8);
                    if wait_res == WAIT_OBJECT_0 {
                        io_pending = false;
                        let mut bytes_transferred = 0u32;
                        if GetOverlappedResult(handle, &overlapped, &mut bytes_transferred, false)
                            .is_ok()
                        {
                            let n = bytes_transferred as usize;
                            if n > 0 {
                                if let Some(report) = parse_hid_report(model, &read_buf[..n]) {
                                    if let Ok(cfg) = config.read() {
                                        engine.process_report(&report, &cfg);
                                    }
                                    if let Ok(mut tel) = telemetry.write() {
                                        tel.touches = report
                                            .touches
                                            .iter()
                                            .copied()
                                            .filter(|t| t.is_down)
                                            .collect();
                                        tel.physical_click = report.physical_click;
                                        tel.emulated_button = engine.active_emulated_button();
                                        let (vx, vy) = engine.velocity();
                                        tel.vel_x = vx;
                                        tel.vel_y = vy;
                                        tel.in_inertia = engine.is_in_inertia();
                                        tel.packet_count += 1;
                                        tel.last_packet_time = Some(Instant::now());
                                    }
                                }
                            }
                        } else {
                            let err = GetLastError();
                            if err != ERROR_IO_INCOMPLETE {
                                break;
                            }
                        }
                    } else if wait_res == WAIT_TIMEOUT {
                        if let Ok(cfg) = config.read() {
                            engine.tick_inertia(&cfg);
                        }
                        if let Ok(mut tel) = telemetry.write() {
                            let (vx, vy) = engine.velocity();
                            tel.vel_x = vx;
                            tel.vel_y = vy;
                            tel.in_inertia = engine.is_in_inertia();
                        }
                    } else {
                        break;
                    }
                }

                if io_pending {
                    let _ = CancelIo(handle);
                }
                let _ = CloseHandle(event);
                let _ = CloseHandle(handle);
            }

            if let Ok(mut st) = status.write() {
                *st = None;
            }
            thread::sleep(Duration::from_millis(1000));
        }
    })
}
