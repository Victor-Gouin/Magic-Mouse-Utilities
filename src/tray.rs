//! Interface System Tray (barre des tâches Windows) ultra-légère via `Shell_NotifyIconW`.
//! Permet de garder l'application active en arrière-plan lorsque la fenêtre est fermée,
//! de rouvrir Magic Mouse Studio instantanément, d'installer les pilotes et de basculer les options.

use crate::config::SharedConfig;
use crate::driver::{spawn_driver_installation, SharedDriverState};
use crate::hid_device::SharedDeviceStatus;
use eframe::egui;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, FindWindowW, GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage,
    RegisterClassW, SetForegroundWindow, ShowWindow, TrackPopupMenu, TranslateMessage,
    CW_USEDEFAULT, IDI_APPLICATION, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED,
    MSG, SW_HIDE, SW_RESTORE, SW_SHOW, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_COMMAND,
    WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP, WM_USER, WNDCLASSW,
    WS_OVERLAPPEDWINDOW,
};

const WM_TRAYICON: u32 = WM_USER + 1;

const IDM_OPEN_GUI: usize = 1000;
const IDM_NATURAL_SCROLL: usize = 1001;
const IDM_INERTIA: usize = 1002;
const IDM_HORIZONTAL_SCROLL: usize = 1003;
const IDM_MIDDLE_CLICK_3F: usize = 1004;
const IDM_MIDDLE_CLICK_ZONE: usize = 1005;
const IDM_INSTALL_DRIVER: usize = 1006;
const IDM_SPEED_SLOW: usize = 1010;
const IDM_SPEED_NORMAL: usize = 1011;
const IDM_SPEED_FAST: usize = 1012;
const IDM_EXIT: usize = 1099;

struct TrayContext {
    running: Arc<AtomicBool>,
    config: SharedConfig,
    status: SharedDeviceStatus,
    driver_state: SharedDriverState,
}

static TRAY_CTX: OnceLock<TrayContext> = OnceLock::new();
static EGUI_CTX: OnceLock<egui::Context> = OnceLock::new();
static TRAY_HWND: OnceLock<isize> = OnceLock::new();
static NOTIFIED_BACKGROUND_ONCE: AtomicBool = AtomicBool::new(false);

/// Enregistre le contexte `egui` pour permettre au System Tray de réafficher la fenêtre instantanément.
pub fn register_egui_context(ctx: egui::Context) {
    let _ = EGUI_CTX.set(ctx);
}

/// Si une instance de l'application tourne déjà en arrière-plan, réaffiche sa fenêtre et renvoie `true`.
pub fn focus_existing_instance_if_running() -> bool {
    unsafe {
        let tray_hwnd = FindWindowW(w!("MagicMouseWinTrayClass"), PCWSTR::null()).unwrap_or_default();
        if !tray_hwnd.0.is_null() {
            restore_gui_window();
            return true;
        }
    }
    false
}

/// Masque la fenêtre graphique pour que l'application reste uniquement en arrière-plan (System Tray).
pub fn hide_gui_window_to_tray(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    unsafe {
        let gui_hwnd = FindWindowW(PCWSTR::null(), w!("Magic Mouse Studio")).unwrap_or_default();
        if !gui_hwnd.0.is_null() {
            let _ = ShowWindow(gui_hwnd, SW_HIDE);
        }

        if !NOTIFIED_BACKGROUND_ONCE.swap(true, Ordering::Relaxed) {
            if let Some(&hwnd_raw) = TRAY_HWND.get() {
                show_balloon_notification(
                    HWND(hwnd_raw as *mut _),
                    "Magic Mouse Studio actif en arrière-plan",
                    "L'application continue de gérer le Multi-Touch et l'inertie. Cliquez sur l'icône de la barre des tâches pour rouvrir.",
                );
            }
        }
    }
}

/// Restaure et met au premier plan la fenêtre graphique `Magic Mouse Studio`.
pub fn restore_gui_window() {
    if let Some(egui_ctx) = EGUI_CTX.get() {
        egui_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        egui_ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        egui_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        egui_ctx.request_repaint();
    }
    unsafe {
        let gui_hwnd = FindWindowW(PCWSTR::null(), w!("Magic Mouse Studio")).unwrap_or_default();
        if !gui_hwnd.0.is_null() {
            let _ = ShowWindow(gui_hwnd, SW_SHOW);
            let _ = ShowWindow(gui_hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(gui_hwnd);
        }
    }
}

fn to_wide_fixed<const N: usize>(s: &str) -> [u16; N] {
    let mut arr = [0u16; N];
    for (i, ch) in s.encode_utf16().take(N - 1).enumerate() {
        arr[i] = ch;
    }
    arr
}

fn to_wide_vec(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

pub fn run_tray_loop(
    running: Arc<AtomicBool>,
    config: SharedConfig,
    status: SharedDeviceStatus,
    driver_state: SharedDriverState,
) {
    let _ = TRAY_CTX.set(TrayContext {
        running: running.clone(),
        config,
        status,
        driver_state,
    });

    unsafe {
        let hinstance = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
        let class_name = w!("MagicMouseWinTrayClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            Default::default(),
            class_name,
            w!("Magic Mouse Utilities (Rust)"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            hinstance,
            None,
        )
        .unwrap_or_default();

        let _ = TRAY_HWND.set(hwnd.0 as isize);

        let hicon = LoadIconW(hinstance, PCWSTR(1usize as *const u16))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default();

        let nid = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            hIcon: hicon,
            szTip: to_wide_fixed::<128>("Magic Mouse Studio — Actif en arrière-plan"),
            ..Default::default()
        };

        let _ = Shell_NotifyIconW(NIM_ADD, &nid);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        running.store(false, Ordering::Relaxed);
    }
}

unsafe fn show_balloon_notification(hwnd: HWND, title: &str, body: &str) {
    let nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_INFO,
        dwInfoFlags: NIIF_INFO,
        szInfoTitle: to_wide_fixed::<64>(title),
        szInfo: to_wide_fixed::<256>(body),
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
}

unsafe fn update_tooltip(hwnd: HWND, tip: &str) {
    let nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_TIP,
        szTip: to_wide_fixed::<128>(tip),
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
}

unsafe fn show_context_menu(hwnd: HWND) {
    let Some(ctx) = TRAY_CTX.get() else { return };

    let hmenu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return,
    };

    // 1. État de connexion de la Magic Mouse
    let status_text = if let Ok(st_guard) = ctx.status.read() {
        if let Some(info) = st_guard.as_ref() {
            let mt_str = if info.multitouch_unlocked {
                "Multi-Touch actif"
            } else {
                "Mode basique (Pilote requis)"
            };
            format!("🟢 {} — {}", info.model.display_name(), mt_str)
        } else {
            "⚪ Magic Mouse non détectée (Recherche...)".to_string()
        }
    } else {
        "⚪ État inconnu".to_string()
    };

    update_tooltip(hwnd, &status_text);

    let status_w = to_wide_vec(&status_text);
    let _ = AppendMenuW(hmenu, MF_STRING | MF_GRAYED, 0, PCWSTR(status_w.as_ptr()));
    let _ = AppendMenuW(
        hmenu,
        MF_STRING,
        IDM_OPEN_GUI,
        w!("Ouvrir Magic Mouse Studio (Interface graphique)..."),
    );
    let _ = AppendMenuW(
        hmenu,
        MF_STRING,
        IDM_INSTALL_DRIVER,
        w!("Installer / Réparer les pilotes Magic Mouse..."),
    );
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());

    // 2. Options de défilement et de clic
    if let Ok(cfg) = ctx.config.read() {
        let flag_nat = if cfg.natural_scrolling {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING | MF_UNCHECKED
        };
        let _ = AppendMenuW(
            hmenu,
            flag_nat,
            IDM_NATURAL_SCROLL,
            w!("Défilement naturel (Inversé style macOS)"),
        );

        let flag_inertia = if cfg.inertia_enabled {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING | MF_UNCHECKED
        };
        let _ = AppendMenuW(
            hmenu,
            flag_inertia,
            IDM_INERTIA,
            w!("Inertie fluide (Momentum Scroll)"),
        );

        let flag_hscroll = if cfg.horizontal_scroll_enabled {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING | MF_UNCHECKED
        };
        let _ = AppendMenuW(
            hmenu,
            flag_hscroll,
            IDM_HORIZONTAL_SCROLL,
            w!("Défilement horizontal (Gauche / Droite)"),
        );

        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());

        let flag_mc3 = if cfg.multi_finger_middle_click {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING | MF_UNCHECKED
        };
        let _ = AppendMenuW(
            hmenu,
            flag_mc3,
            IDM_MIDDLE_CLICK_3F,
            w!("Clic Milieu à 3 doigts posés"),
        );

        let flag_mcz = if cfg.middle_click_zone_enabled {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING | MF_UNCHECKED
        };
        let _ = AppendMenuW(
            hmenu,
            flag_mcz,
            IDM_MIDDLE_CLICK_ZONE,
            w!("Clic Milieu sur la bande centrale (1 doigt)"),
        );

        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());

        let is_slow = (cfg.scroll_speed_vertical - 0.85).abs() < 0.15;
        let is_norm = (cfg.scroll_speed_vertical - 1.35).abs() < 0.15;
        let is_fast = cfg.scroll_speed_vertical >= 1.8;

        let _ = AppendMenuW(
            hmenu,
            if is_slow { MF_STRING | MF_CHECKED } else { MF_STRING },
            IDM_SPEED_SLOW,
            w!("Vitesse de défilement : Douce (0.85x)"),
        );
        let _ = AppendMenuW(
            hmenu,
            if is_norm { MF_STRING | MF_CHECKED } else { MF_STRING },
            IDM_SPEED_NORMAL,
            w!("Vitesse de défilement : Normale (1.35x)"),
        );
        let _ = AppendMenuW(
            hmenu,
            if is_fast { MF_STRING | MF_CHECKED } else { MF_STRING },
            IDM_SPEED_FAST,
            w!("Vitesse de défilement : Rapide (2.0x)"),
        );
    }

    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_EXIT, w!("Quitter Magic Mouse Win"));

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let _ = TrackPopupMenu(
        hmenu,
        TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
        pt.x,
        pt.y,
        0,
        hwnd,
        None,
    );
    let _ = DestroyMenu(hmenu);
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAYICON => {
            let event = (lparam.0 & 0xFFFF) as u32;
            if event == WM_LBUTTONDBLCLK {
                restore_gui_window();
            } else if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                show_context_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd_id = wparam.0 & 0xFFFF;
            if let Some(ctx) = TRAY_CTX.get() {
                match cmd_id {
                    IDM_OPEN_GUI => {
                        restore_gui_window();
                    }
                    IDM_INSTALL_DRIVER => {
                        spawn_driver_installation(ctx.driver_state.clone());
                        restore_gui_window();
                    }
                    IDM_NATURAL_SCROLL => {
                        if let Ok(mut cfg) = ctx.config.write() {
                            cfg.natural_scrolling = !cfg.natural_scrolling;
                            let _ = cfg.save();
                        }
                    }
                    IDM_INERTIA => {
                        if let Ok(mut cfg) = ctx.config.write() {
                            cfg.inertia_enabled = !cfg.inertia_enabled;
                            let _ = cfg.save();
                        }
                    }
                    IDM_HORIZONTAL_SCROLL => {
                        if let Ok(mut cfg) = ctx.config.write() {
                            cfg.horizontal_scroll_enabled = !cfg.horizontal_scroll_enabled;
                            let _ = cfg.save();
                        }
                    }
                    IDM_MIDDLE_CLICK_3F => {
                        if let Ok(mut cfg) = ctx.config.write() {
                            cfg.multi_finger_middle_click = !cfg.multi_finger_middle_click;
                            let _ = cfg.save();
                        }
                    }
                    IDM_MIDDLE_CLICK_ZONE => {
                        if let Ok(mut cfg) = ctx.config.write() {
                            cfg.middle_click_zone_enabled = !cfg.middle_click_zone_enabled;
                            let _ = cfg.save();
                        }
                    }
                    IDM_SPEED_SLOW => {
                        if let Ok(mut cfg) = ctx.config.write() {
                            cfg.scroll_speed_vertical = 0.85;
                            cfg.scroll_speed_horizontal = 0.75;
                            let _ = cfg.save();
                        }
                    }
                    IDM_SPEED_NORMAL => {
                        if let Ok(mut cfg) = ctx.config.write() {
                            cfg.scroll_speed_vertical = 1.35;
                            cfg.scroll_speed_horizontal = 1.15;
                            let _ = cfg.save();
                        }
                    }
                    IDM_SPEED_FAST => {
                        if let Ok(mut cfg) = ctx.config.write() {
                            cfg.scroll_speed_vertical = 2.0;
                            cfg.scroll_speed_horizontal = 1.6;
                            let _ = cfg.save();
                        }
                    }
                    IDM_EXIT => {
                        ctx.running.store(false, Ordering::Relaxed);
                        let nid = NOTIFYICONDATAW {
                            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                            hWnd: hwnd,
                            uID: 1,
                            ..Default::default()
                        };
                        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
                        if let Some(egui_ctx) = EGUI_CTX.get() {
                            egui_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            egui_ctx.request_repaint();
                        }
                        let _ = DestroyWindow(hwnd);
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
