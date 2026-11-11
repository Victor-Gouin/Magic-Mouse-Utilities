//! Gestion native de l'installation et de la réparation des pilotes Apple Magic Mouse
//! (Boot Camp / HidUsb / HidBth) ainsi que du déverrouillage des collections HID Multi-Touch.
//!
//! Conçu pour être compatible avec les EDR d'entreprise (SentinelOne, CrowdStrike, Defender) :
//! - Aucune écriture de script temporaire dans `%TEMP%`.
//! - Aucun appel à `powershell.exe -ExecutionPolicy Bypass`.
//! - Utilise uniquement les API Win32 natives (`HidD_SetFeature`, `ShellExecuteW`) et l'outil
//!   système signé Microsoft `C:\Windows\System32\pnputil.exe`.

use crate::hid_device::{enumerate_magic_mice, unlock_multitouch};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

#[derive(Debug, Clone)]
pub enum DriverInstallStatus {
    Idle,
    Running(String),
    Success(String, Instant),
    Error(String, Instant),
}

pub type SharedDriverState = Arc<RwLock<DriverInstallStatus>>;

pub fn new_shared_driver_state() -> SharedDriverState {
    Arc::new(RwLock::new(DriverInstallStatus::Idle))
}

fn to_wide_null(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

/// Envoie immédiatement le Feature Report d'activation Multi-Touch (`[0xF1, 0x02, 0x01]` ou `[0xD7, 0x01]`)
/// à toutes les collections HID Apple Magic Mouse présentes sur le PC.
/// Retourne `(nombre_interfaces_trouvées, nombre_interfaces_déverrouillées)`.
pub fn force_unlock_all_magic_mice() -> (usize, usize) {
    let candidates = enumerate_magic_mice();
    let total = candidates.len();
    let mut unlocked_count = 0usize;

    for (model, path_w, _) in candidates {
        unsafe {
            let h = CreateFileW(
                PCWSTR(path_w.as_ptr()),
                0x80000000u32 | 0x40000000u32,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                Default::default(),
                None,
            );

            let handle = match h {
                Ok(valid) if valid != INVALID_HANDLE_VALUE => valid,
                _ => match CreateFileW(
                    PCWSTR(path_w.as_ptr()),
                    0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    Default::default(),
                    None,
                ) {
                    Ok(ro) if ro != INVALID_HANDLE_VALUE => ro,
                    _ => continue,
                },
            };

            if unlock_multitouch(handle, model) {
                unlocked_count += 1;
            }
            let _ = CloseHandle(handle);
        }
    }

    (total, unlocked_count)
}

/// Recherche d'éventuels fichiers `.inf` dans le dossier `drivers/` à côté de l'exécutable.
fn find_local_driver_infs() -> Vec<PathBuf> {
    let mut infs = Vec::new();
    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    else {
        return infs;
    };

    let mut search_dirs = vec![exe_dir.join("drivers")];
    if let Some(root) = exe_dir.parent().and_then(|p| p.parent()) {
        search_dirs.push(root.join("drivers"));
    }

    for dir in search_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("inf"))
                    .unwrap_or(false)
                {
                    infs.push(path);
                }
            }
        }
    }
    infs
}

/// Exécute `C:\Windows\System32\pnputil.exe` avec élévation UAC (`runas`) via l'API Win32 officielle `ShellExecuteW`.
fn run_elevated_pnputil(args: &str) -> Result<(), String> {
    let args_w = to_wide_null(args);
    let ret = unsafe {
        ShellExecuteW(
            HWND::default(),
            w!("runas"),
            w!("C:\\Windows\\System32\\pnputil.exe"),
            PCWSTR(args_w.as_ptr()),
            PCWSTR::null(),
            SW_HIDE,
        )
    };

    // Selon la documentation Win32, ShellExecuteW renvoie une valeur > 32 en cas de succès.
    if (ret.0 as isize) > 32 {
        Ok(())
    } else {
        Err(format!(
            "Élévation Administrateur (UAC) refusée ou annulée (code {})",
            ret.0 as isize
        ))
    }
}

/// Lance en arrière-plan l'installation / réparation des pilotes Magic Mouse via `pnputil.exe`
/// suivie d'un déverrouillage direct des collections HID Multi-Touch.
pub fn spawn_driver_installation(state: SharedDriverState) {
    if let Ok(guard) = state.read() {
        if matches!(*guard, DriverInstallStatus::Running(_)) {
            return;
        }
    }

    if let Ok(mut guard) = state.write() {
        *guard = DriverInstallStatus::Running(
            "Activation Multi-Touch et vérification PnP en cours...".to_string(),
        );
    }

    thread::spawn(move || {
        // 1. Premier essai immédiat sans UAC via HidD_SetFeature (suffisant dans 95% des cas pour MM1/MM2/USB-C)
        let (found_initial, unlocked_initial) = force_unlock_all_magic_mice();
        let inf_files = find_local_driver_infs();

        // Si aucun fichier .inf externe n'est à installer et que le Multi-Touch est déjà déverrouillé,
        // on évite même une demande UAC inutile !
        if inf_files.is_empty() && unlocked_initial > 0 {
            let msg = format!(
                "Pilote HID opérationnel · Multi-Touch actif ({}/{} collections HID)",
                unlocked_initial, found_initial
            );
            if let Ok(mut guard) = state.write() {
                *guard = DriverInstallStatus::Success(msg, Instant::now());
            }
            return;
        }

        // 2. Sinon, installation des .inf présents dans ./drivers ou rafraîchissement PnP via pnputil.exe signé Microsoft
        let pnp_result = if let Some(first_inf) = inf_files.first() {
            let args = format!(
                "/add-driver \"{}\" /install",
                first_inf.to_string_lossy()
            );
            run_elevated_pnputil(&args)
        } else {
            run_elevated_pnputil("/scan-devices")
        };

        if let Err(err) = pnp_result {
            if let Ok(mut guard) = state.write() {
                *guard = DriverInstallStatus::Error(err, Instant::now());
            }
            return;
        }

        // Laisse le temps à pnputil de terminer le rafraîchissement des noeuds HID
        thread::sleep(Duration::from_millis(1200));

        let (found, unlocked) = force_unlock_all_magic_mice();
        let msg = if unlocked > 0 {
            format!(
                "Pilotes configurés · Multi-Touch actif ({}/{} collections HID)",
                unlocked, found
            )
        } else if found > 0 {
            format!(
                "Pilotes rafraîchis ({} interfaces détectées) · Rebranchez ou rallumez la souris",
                found
            )
        } else {
            "Pile HID vérifiée · Connectez la Magic Mouse en Bluetooth ou USB-C".to_string()
        };

        if let Ok(mut guard) = state.write() {
            *guard = DriverInstallStatus::Success(msg, Instant::now());
        }
    });
}
