// Masque la console en mode Release pour tourner silencieusement
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod config;
pub mod driver;
pub mod gestures;
pub mod gui;
pub mod hid_device;
pub mod injector;
pub mod protocol;
pub mod tray;

use config::{AppConfig, SharedConfig};
use driver::new_shared_driver_state;
use gestures::{GestureEngine, RecordingSink};
use hid_device::{
    enumerate_magic_mice, spawn_hid_worker, LiveTelemetry, SharedDeviceStatus, SharedTelemetry,
};
use protocol::{
    parse_hid_report, MagicMouseModel, MOUSE2_REPORT_ID, TOUCH_STATE_DRAG, TOUCH_STATE_NONE,
    TOUCH_STATE_START,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;

/// Encode un point tactile Magic Mouse 2 (coordonnées 12 bits signés) en 8 octets bruts `tdata[0..8]`
/// exactement comme le firmware de la Magic Mouse 2 (Lightning 0x0269 / USB-C 0x0323).
fn encode_mm2_touch_bytes(id: u8, x: i32, y: i32, state: u8) -> [u8; 8] {
    let ux = (x & 0x0FFF) as u16;
    let uy = ((-y) & 0x0FFF) as u16;

    let t0 = (ux & 0xFF) as u8;
    let t1 = (((ux >> 8) & 0x0F) | ((uy & 0x0F) << 4)) as u8;
    let t2 = ((uy >> 4) & 0xFF) as u8;
    let t3 = 24u8;
    let t4 = 16u8;
    let t5 = ((id & 0x03) << 6) | 12u8;
    let t6 = ((id >> 2) & 0x03) | (32u8 << 2);
    let t7 = state;

    [t0, t1, t2, t3, t4, t5, t6, t7]
}

fn run_hardware_simulation() {
    println!("=== Simulation Matérielle Apple Magic Mouse 2 (Sans souris physique) ===");
    let model = MagicMouseModel::MagicMouse2UsbC;
    let cfg = AppConfig::default();
    let mut engine = GestureEngine::new();
    let mut sink = RecordingSink::default();

    println!(
        "[1] Modèle simulé : {} (Feature Report Multi-Touch : {:02X?})",
        model.display_name(),
        model.multitouch_feature_report()
    );

    let frames_y = [
        (0, TOUCH_STATE_START),
        (25, TOUCH_STATE_DRAG),
        (65, TOUCH_STATE_DRAG),
        (115, TOUCH_STATE_DRAG),
        (170, TOUCH_STATE_DRAG),
        (170, TOUCH_STATE_NONE),
    ];

    for (step, &(y, state)) in frames_y.iter().enumerate() {
        let mut raw_packet = vec![0u8; 14];
        raw_packet[0] = MOUSE2_REPORT_ID;
        raw_packet.extend_from_slice(&encode_mm2_touch_bytes(1, -100, y, state));

        let report = parse_hid_report(model, &raw_packet).expect("Paquet HID 0x12 valide");
        let tp = &report.touches[0];
        println!(
            "  Trame #{step} -> décodé HID : id={}, x={}, y={}, state=0x{:02X}, down={}",
            tp.id, tp.x, tp.y, tp.state, tp.is_down
        );
        engine.process_report_with_sink(&report, &cfg, &mut sink);
    }

    let scroll_before_inertia = sink.events.len();
    println!(
        "  -> Événements Scroll directs générés : {} ({:?})",
        scroll_before_inertia, sink.events
    );

    let mut inertia_ticks = 0;
    while engine.is_in_inertia() && inertia_ticks < 60 {
        engine.tick_inertia_with_sink(&cfg, &mut sink, true);
        inertia_ticks += 1;
    }
    println!(
        "  -> Inertie cinétique : {} ticks exécutés, {} événements Scroll supplémentaires générés.",
        inertia_ticks,
        sink.events.len() - scroll_before_inertia
    );

    sink.events.clear();
    for &click in &[true, false] {
        let mut raw_packet = vec![0u8; 14];
        raw_packet[0] = MOUSE2_REPORT_ID;
        raw_packet[1] = if click { 0x01 } else { 0x00 };
        raw_packet.extend_from_slice(&encode_mm2_touch_bytes(2, 420, 80, TOUCH_STATE_DRAG));

        let report = parse_hid_report(model, &raw_packet).unwrap();
        engine.process_report_with_sink(&report, &cfg, &mut sink);
    }
    println!("[2] Simulation Clic Droit tactile (X=+420) -> {:?}", sink.events);

    sink.events.clear();
    for &click in &[true, false] {
        let mut raw_packet = vec![0u8; 14];
        raw_packet[0] = MOUSE2_REPORT_ID;
        raw_packet[1] = if click { 0x01 } else { 0x00 };
        raw_packet.extend_from_slice(&encode_mm2_touch_bytes(1, -350, 90, TOUCH_STATE_DRAG));
        raw_packet.extend_from_slice(&encode_mm2_touch_bytes(2, 0, 140, TOUCH_STATE_DRAG));
        raw_packet.extend_from_slice(&encode_mm2_touch_bytes(3, 350, 90, TOUCH_STATE_DRAG));

        let report = parse_hid_report(model, &raw_packet).unwrap();
        engine.process_report_with_sink(&report, &cfg, &mut sink);
    }
    println!("[3] Simulation Clic Milieu (3 doigts) -> {:?}", sink.events);
    println!("=== Simulation terminée avec succès ===");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Mode diagnostic en ligne de commande : `magic-mouse-win.exe --diagnose`
    if args.iter().any(|a| a == "--diagnose") {
        println!("=== Diagnostic HID Apple Magic Mouse ===");
        let devices = enumerate_magic_mice();
        if devices.is_empty() {
            println!("Aucune Magic Mouse détectée dans l'arbre HID Windows.");
            println!("Vérifiez que la souris est connectée en Bluetooth ou via câble USB-C/Lightning.");
        } else {
            for (idx, (model, _, path)) in devices.iter().enumerate() {
                println!("[{}] Modèle : {}", idx + 1, model.display_name());
                println!("    Chemin : {}", path);
            }
        }
        return;
    }

    // Mode simulation matérielle sans souris physique : `magic-mouse-win.exe --simulate`
    if args.iter().any(|a| a == "--simulate") {
        run_hardware_simulation();
        return;
    }

    // Si une instance tourne déjà en arrière-plan dans le System Tray, réaffiche sa fenêtre
    if tray::focus_existing_instance_if_running() {
        return;
    }

    let start_in_tray = args.iter().any(|a| a == "--tray");
    let running = Arc::new(AtomicBool::new(true));
    let config: SharedConfig = Arc::new(RwLock::new(AppConfig::load()));
    let status: SharedDeviceStatus = Arc::new(RwLock::new(None));
    let telemetry: SharedTelemetry = Arc::new(RwLock::new(LiveTelemetry::default()));
    let driver_state = new_shared_driver_state();

    // Démarre le thread HID temps-réel (lecture Overlapped + inertie 125 Hz)
    let _worker_handle = spawn_hid_worker(
        running.clone(),
        config.clone(),
        status.clone(),
        telemetry.clone(),
    );

    // Lance l'icône System Tray en arrière-plan pour maintenir le programme actif même fenêtre fermée
    let tray_running = running.clone();
    let tray_config = config.clone();
    let tray_status = status.clone();
    let tray_driver_state = driver_state.clone();
    thread::spawn(move || {
        tray::run_tray_loop(tray_running, tray_config, tray_status, tray_driver_state);
    });

    // Lance l'interface graphique (la fermeture [X] masque simplement la fenêtre vers le System Tray)
    let _ = gui::run_gui(
        running.clone(),
        config,
        status,
        telemetry,
        driver_state,
        start_in_tray,
    );
    running.store(false, Ordering::Relaxed);
}
