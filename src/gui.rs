//! Interface graphique native sobre, précise et matérielle (`egui` / `eframe`).
//! Palette "Atelier Aluminium & Graphite" :
//! - Fond principal : `#111214` (Obsidienne mate)
//! - Panneau latéral / surface : `#17191C` (Graphite)
//! - Bordures fines 1px : `#26292E`
//! - Texte primaire : `#F2F4F7` / secondaire : `#8E95A2`
//! - Accent matériel : `#38BDF8` (Bleu optique précis) et `#10B981` (Émeraude de liaison)

use crate::config::{AppConfig, SharedConfig};
use crate::driver::{
    force_unlock_all_magic_mice, spawn_driver_installation, DriverInstallStatus, SharedDriverState,
};
use crate::hid_device::{SharedDeviceStatus, SharedTelemetry};
use crate::injector::EmulatedButton;
use crate::protocol::{TouchPoint, TOUCH_STATE_DRAG};
use crate::tray::{hide_gui_window_to_tray, register_egui_context};
use eframe::egui::{
    self, Align, Color32, FontFamily, FontId, Frame, Layout, Margin, Pos2, Rect, RichText,
    Rounding, Sense, Stroke, Vec2,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

// Palette de couleurs intentionnelle (pas de dégradés génériques ni de cartes SaaS redondantes)
const BG_OBSIDIAN: Color32 = Color32::from_rgb(0x11, 0x12, 0x14);
const SURFACE_GRAPHITE: Color32 = Color32::from_rgb(0x17, 0x19, 0x1C);
const SURFACE_ELEVATED: Color32 = Color32::from_rgb(0x1E, 0x20, 0x25);
const BORDER_SUBTLE: Color32 = Color32::from_rgb(0x26, 0x29, 0x2E);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xF2, 0xF4, 0xF7);
const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x8E, 0x95, 0xA2);
const TEXT_MUTED: Color32 = Color32::from_rgb(0x5C, 0x62, 0x6E);
const ACCENT_OPTICAL: Color32 = Color32::from_rgb(0x38, 0xBD, 0xF8);
const ACCENT_EMERALD: Color32 = Color32::from_rgb(0x10, 0xB9, 0x81);
const ACCENT_AMBER: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
const ACCENT_CORAL: Color32 = Color32::from_rgb(0xEF, 0x44, 0x44);

pub struct MagicMouseGuiApp {
    running: Arc<AtomicBool>,
    config: SharedConfig,
    status: SharedDeviceStatus,
    telemetry: SharedTelemetry,
    driver_state: SharedDriverState,
    start_hidden: bool,
    hidden_on_startup_done: bool,
    /// Permet de tester et visualiser les gestes sur le radar même sans la souris physique
    demo_mode: bool,
    demo_fingers_count: usize,
    demo_pos: Vec2,
    demo_vel: Vec2,
    demo_click: bool,
    last_frame: Instant,
    status_toast: Option<(String, Instant)>,
}

impl MagicMouseGuiApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        running: Arc<AtomicBool>,
        config: SharedConfig,
        status: SharedDeviceStatus,
        telemetry: SharedTelemetry,
        driver_state: SharedDriverState,
        start_hidden: bool,
    ) -> Self {
        Self::configure_style(&cc.egui_ctx);
        register_egui_context(cc.egui_ctx.clone());

        let is_connected = status.read().map(|s| s.is_some()).unwrap_or(false);

        Self {
            running,
            config,
            status,
            telemetry,
            driver_state,
            start_hidden,
            hidden_on_startup_done: false,
            demo_mode: !is_connected,
            demo_fingers_count: 1,
            demo_pos: Vec2::new(0.0, 120.0),
            demo_vel: Vec2::ZERO,
            demo_click: false,
            last_frame: Instant::now(),
            status_toast: None,
        }
    }

    fn configure_style(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG_OBSIDIAN;
        visuals.window_fill = BG_OBSIDIAN;
        visuals.extreme_bg_color = Color32::from_rgb(0x0B, 0x0C, 0x0E);
        visuals.faint_bg_color = SURFACE_GRAPHITE;

        visuals.widgets.noninteractive.bg_fill = SURFACE_GRAPHITE;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
        visuals.widgets.noninteractive.rounding = Rounding::same(6.0);

        visuals.widgets.inactive.bg_fill = SURFACE_ELEVATED;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
        visuals.widgets.inactive.rounding = Rounding::same(6.0);

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x25, 0x29, 0x30);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_OPTICAL);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
        visuals.widgets.hovered.rounding = Rounding::same(6.0);

        visuals.widgets.active.bg_fill = ACCENT_OPTICAL;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT_OPTICAL);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, BG_OBSIDIAN);
        visuals.widgets.active.rounding = Rounding::same(6.0);

        visuals.selection.bg_fill = ACCENT_OPTICAL.gamma_multiply(0.28);
        visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT_OPTICAL);

        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(10.0, 10.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        ctx.set_style(style);
    }

    fn save_config(&mut self, cfg: &AppConfig) {
        if cfg.save().is_ok() {
            self.status_toast = Some(("Réglages enregistrés".to_string(), Instant::now()));
        }
    }

    /// Dessine un interrupteur à bascule sobre de style matériel.
    fn draw_toggle_row(
        ui: &mut egui::Ui,
        title: &str,
        subtitle: &str,
        value: &mut bool,
    ) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(title)
                        .size(13.5)
                        .color(TEXT_PRIMARY)
                        .strong(),
                );
                ui.label(RichText::new(subtitle).size(11.5).color(TEXT_SECONDARY));
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let desired_size = Vec2::new(40.0, 22.0);
                let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());
                if response.clicked() {
                    *value = !*value;
                    changed = true;
                }

                let anim = ui.ctx().animate_bool_responsive(response.id, *value);
                let bg = Color32::from_rgb(
                    (0x22 as f32 + (0x38 - 0x22) as f32 * anim) as u8,
                    (0x25 as f32 + (0xBD - 0x25) as f32 * anim) as u8,
                    (0x2B as f32 + (0xF8 - 0x2B) as f32 * anim) as u8,
                );
                ui.painter()
                    .rect_filled(rect, Rounding::same(11.0), bg);

                let circle_x = egui::lerp((rect.left() + 11.0)..=(rect.right() - 11.0), anim);
                let knob_color = if *value {
                    Color32::from_rgb(0x0B, 0x0C, 0x0E)
                } else {
                    TEXT_SECONDARY
                };
                ui.painter().circle_filled(
                    Pos2::new(circle_x, rect.center().y),
                    7.5,
                    knob_color,
                );
            });
        });
        changed
    }

    /// Dessine la silhouette à l'échelle de la Magic Mouse 2, les zones de clic (Gauche / Milieu / Droite),
    /// les points de contact capacitifs et le vecteur d'inertie.
    fn draw_capacitive_surface(&mut self, ui: &mut egui::Ui, cfg: &AppConfig) {
        let available = ui.available_size();
        let panel_rect = ui.allocate_space(Vec2::new(available.x, available.y)).1;
        let painter = ui.painter_at(panel_rect);

        // Cadre de fond du banc d'essai capacitif
        painter.rect(
            panel_rect,
            Rounding::same(10.0),
            SURFACE_GRAPHITE,
            Stroke::new(1.0_f32, BORDER_SUBTLE),
        );

        // En-tête du radar
        let header_pos = Pos2::new(panel_rect.left() + 18.0, panel_rect.top() + 16.0);
        painter.text(
            header_pos,
            egui::Align2::LEFT_TOP,
            "Surface capacitive",
            FontId::new(14.0, FontFamily::Proportional),
            TEXT_PRIMARY,
        );

        let sub_text = if self.demo_mode {
            "Mode aperçu interactif : glissez sur la coque pour tester le scroll et les zones"
        } else {
            "Lecture directe des coordonnées 12 bits du capteur Multi-Touch"
        };
        painter.text(
            Pos2::new(panel_rect.left() + 18.0, panel_rect.top() + 35.0),
            egui::Align2::LEFT_TOP,
            sub_text,
            FontId::new(11.5, FontFamily::Proportional),
            TEXT_SECONDARY,
        );

        // Géométrie de la silhouette Magic Mouse au centre
        let mouse_w = 186.0;
        let mouse_h = (panel_rect.height() - 140.0).clamp(260.0, 340.0);
        let center = Pos2::new(panel_rect.center().x, panel_rect.center().y + 6.0);
        let mouse_rect = Rect::from_center_size(center, Vec2::new(mouse_w, mouse_h));

        // Interaction souris en mode Aperçu Interactif (sans matériel)
        let interact_resp = ui.interact(
            mouse_rect,
            egui::Id::new("mm_surface_canvas"),
            Sense::click_and_drag(),
        );
        let dt = self.last_frame.elapsed().as_secs_f32().clamp(0.001, 0.05);
        self.last_frame = Instant::now();

        if self.demo_mode {
            if interact_resp.dragged() || interact_resp.is_pointer_button_down_on() {
                if let Some(ptr) = interact_resp.interact_pointer_pos() {
                    let norm_x =
                        ((ptr.x - mouse_rect.center().x) / (mouse_w * 0.5)).clamp(-0.92, 0.92);
                    let norm_y =
                        -((ptr.y - mouse_rect.center().y) / (mouse_h * 0.5)).clamp(-0.92, 0.92);
                    let new_pos = Vec2::new(norm_x * 1100.0, norm_y * 1500.0);
                    let delta = new_pos - self.demo_pos;
                    self.demo_vel = self.demo_vel * 0.4 + delta * 0.6;
                    self.demo_pos = new_pos;
                    self.demo_click = ui.input(|i| {
                        i.pointer.secondary_down()
                            || i.pointer
                                .button_double_clicked(egui::PointerButton::Primary)
                    });
                }
            } else {
                self.demo_vel *= cfg.inertia_decay;
                if self.demo_vel.length() < 0.3 {
                    self.demo_vel = Vec2::ZERO;
                }
                self.demo_click = false;
            }
            if self.demo_vel.length() > 0.3 {
                ui.ctx().request_repaint();
            }
        }

        // Ombre portée sobre sous la souris
        let shadow_rect = mouse_rect.translate(Vec2::new(0.0, 8.0)).expand(2.0);
        painter.rect_filled(
            shadow_rect,
            Rounding::same(93.0),
            Color32::from_black_alpha(95),
        );

        // Coque acrylique sombre de la Magic Mouse
        let body_fill = Color32::from_rgb(0x13, 0x15, 0x19);
        painter.rect(
            mouse_rect,
            Rounding::same(93.0),
            body_fill,
            Stroke::new(1.5_f32, Color32::from_rgb(0x36, 0x3B, 0x44)),
        );

        // Reflet supérieur discret sur l'arc avant de la souris
        let top_highlight = Rect::from_min_max(
            Pos2::new(mouse_rect.left() + 26.0, mouse_rect.top() + 3.0),
            Pos2::new(mouse_rect.right() - 26.0, mouse_rect.top() + 5.0),
        );
        painter.rect_filled(
            top_highlight,
            Rounding::same(2.0),
            Color32::from_white_alpha(22),
        );

        // Visualisation des zones de clic sur la moitié supérieure de la coque
        let zone_top = mouse_rect.top() + 26.0;
        let zone_bottom = mouse_rect.center().y - 10.0;

        let right_thresh_ratio =
            (cfg.right_click_zone_threshold_x as f32 / 1100.0).clamp(-0.8, 0.85);
        let right_split_x = mouse_rect.center().x + right_thresh_ratio * (mouse_w * 0.5);

        let right_zone_rect = Rect::from_min_max(
            Pos2::new(right_split_x, zone_top),
            Pos2::new(mouse_rect.right() - 14.0, zone_bottom),
        );
        painter.rect(
            right_zone_rect,
            Rounding::same(18.0),
            ACCENT_OPTICAL.gamma_multiply(0.05),
            Stroke::new(1.0_f32, ACCENT_OPTICAL.gamma_multiply(0.32)),
        );
        painter.text(
            Pos2::new(right_zone_rect.center().x, right_zone_rect.top() + 14.0),
            egui::Align2::CENTER_TOP,
            "Clic droit",
            FontId::new(10.5, FontFamily::Proportional),
            ACCENT_OPTICAL.gamma_multiply(0.85),
        );

        if cfg.middle_click_enabled && cfg.middle_click_zone_enabled {
            let half_w_ratio =
                (cfg.middle_click_zone_half_width as f32 / 1100.0).clamp(0.05, 0.45);
            let mid_half_px = half_w_ratio * (mouse_w * 0.5);
            let mid_rect = Rect::from_min_max(
                Pos2::new(mouse_rect.center().x - mid_half_px, zone_top),
                Pos2::new(mouse_rect.center().x + mid_half_px, zone_bottom),
            );
            painter.rect(
                mid_rect,
                Rounding::same(10.0),
                ACCENT_EMERALD.gamma_multiply(0.07),
                Stroke::new(1.0_f32, ACCENT_EMERALD.gamma_multiply(0.45)),
            );
            painter.text(
                Pos2::new(mid_rect.center().x, mid_rect.bottom() - 16.0),
                egui::Align2::CENTER_TOP,
                "Milieu",
                FontId::new(10.0, FontFamily::Proportional),
                ACCENT_EMERALD,
            );
        }

        painter.line_segment(
            [
                Pos2::new(mouse_rect.left() + 18.0, mouse_rect.center().y),
                Pos2::new(mouse_rect.right() - 18.0, mouse_rect.center().y),
            ],
            Stroke::new(1.0_f32, Color32::from_white_alpha(10)),
        );
        painter.line_segment(
            [
                Pos2::new(mouse_rect.center().x, mouse_rect.top() + 24.0),
                Pos2::new(mouse_rect.center().x, mouse_rect.bottom() - 24.0),
            ],
            Stroke::new(1.0_f32, Color32::from_white_alpha(10)),
        );

        let (touches, vel_x, vel_y, in_inertia, emu_btn) = if self.demo_mode {
            let mut pts = Vec::new();
            let spread = 240.0;
            for i in 0..self.demo_fingers_count {
                let offset_x = match self.demo_fingers_count {
                    1 => 0.0,
                    2 => (i as f32 - 0.5) * spread,
                    _ => (i as f32 - 1.0) * spread,
                };
                pts.push(TouchPoint {
                    id: (i + 1) as u8,
                    x: (self.demo_pos.x + offset_x).clamp(-1050.0, 1050.0) as i32,
                    y: self.demo_pos.y as i32,
                    touch_major: 24,
                    touch_minor: 18,
                    size: 14,
                    orientation: 0,
                    state: TOUCH_STATE_DRAG,
                    is_down: true,
                });
            }
            let btn = if self.demo_fingers_count >= 3 && cfg.multi_finger_middle_click {
                EmulatedButton::Middle
            } else if cfg.middle_click_zone_enabled
                && (self.demo_pos.x.abs() as i32) <= cfg.middle_click_zone_half_width
            {
                EmulatedButton::Middle
            } else if (self.demo_pos.x as i32) >= cfg.right_click_zone_threshold_x {
                EmulatedButton::Right
            } else {
                EmulatedButton::None
            };
            (
                pts,
                self.demo_vel.x,
                self.demo_vel.y,
                self.demo_vel.length() > 1.5,
                btn,
            )
        } else if let Ok(tel) = self.telemetry.read() {
            (
                tel.touches.clone(),
                tel.vel_x,
                tel.vel_y,
                tel.in_inertia,
                tel.emulated_button,
            )
        } else {
            (Vec::new(), 0.0, 0.0, false, EmulatedButton::None)
        };

        for tp in &touches {
            let px = mouse_rect.center().x + (tp.x as f32 / 1100.0) * (mouse_w * 0.5);
            let py = mouse_rect.center().y - (tp.y as f32 / 1500.0) * (mouse_h * 0.5);
            let pt = Pos2::new(
                px.clamp(mouse_rect.left() + 16.0, mouse_rect.right() - 16.0),
                py.clamp(mouse_rect.top() + 16.0, mouse_rect.bottom() - 16.0),
            );

            let halo_radius = 14.0 + (tp.size as f32 * 0.35);
            painter.circle_filled(pt, halo_radius, ACCENT_OPTICAL.gamma_multiply(0.18));
            painter.circle_stroke(
                pt,
                halo_radius,
                Stroke::new(1.0_f32, ACCENT_OPTICAL.gamma_multiply(0.55)),
            );
            painter.circle_filled(pt, 5.5, ACCENT_OPTICAL);

            if vel_x.abs() > 0.8 || vel_y.abs() > 0.8 {
                let arrow_end = Pos2::new(
                    pt.x + (vel_x * 0.45).clamp(-45.0, 45.0),
                    pt.y - (vel_y * 0.45).clamp(-45.0, 45.0),
                );
                painter.line_segment([pt, arrow_end], Stroke::new(2.0_f32, ACCENT_EMERALD));
            }
        }

        let footer_y = panel_rect.bottom() - 56.0;
        let telem_str = if let Some(first) = touches.first() {
            let zone_label = match emu_btn {
                EmulatedButton::Right => "Zone : Clic droit",
                EmulatedButton::Middle => "Zone : Clic milieu",
                EmulatedButton::None => "Zone : Clic gauche / Scroll",
            };
            format!(
                "X {:+4}   Y {:+4}   ·   {} doigt(s)   ·   {}",
                first.x,
                first.y,
                touches.len(),
                zone_label
            )
        } else if in_inertia {
            format!("Inertie cinétique active (Vy: {:+.1})", vel_y)
        } else {
            "Aucun contact détecté sur la surface".to_string()
        };

        painter.text(
            Pos2::new(panel_rect.center().x, footer_y),
            egui::Align2::CENTER_TOP,
            telem_str,
            FontId::new(11.5, FontFamily::Monospace),
            TEXT_SECONDARY,
        );

        let controls_rect = Rect::from_min_max(
            Pos2::new(panel_rect.left() + 16.0, panel_rect.bottom() - 34.0),
            Pos2::new(panel_rect.right() - 16.0, panel_rect.bottom() - 10.0),
        );
        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(controls_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child_ui.checkbox(
            &mut self.demo_mode,
            RichText::new("Simulateur interactif")
                .size(11.5)
                .color(TEXT_SECONDARY),
        );
        if self.demo_mode {
            child_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for count in [3usize, 2, 1] {
                    let selected = self.demo_fingers_count == count;
                    let label = format!("{} doigt{}", count, if count > 1 { "s" } else { "" });
                    if ui
                        .selectable_label(
                            selected,
                            RichText::new(label).size(11.0).color(if selected {
                                TEXT_PRIMARY
                            } else {
                                TEXT_SECONDARY
                            }),
                        )
                        .clicked()
                    {
                        self.demo_fingers_count = count;
                    }
                }
            });
        }
        let _ = dt;
    }
}

impl eframe::App for MagicMouseGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Si démarré avec `--tray`, masque immédiatement la fenêtre au premier rendu
        if self.start_hidden && !self.hidden_on_startup_done {
            self.hidden_on_startup_done = true;
            hide_gui_window_to_tray(ctx);
        }

        // Intercepte la fermeture de la fenêtre [X] pour garder l'application active en arrière-plan (System Tray)
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.running.load(Ordering::Relaxed) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                hide_gui_window_to_tray(ctx);
                return;
            }
        }

        let device_info = self.status.read().ok().and_then(|g| g.clone());
        let drv_status = self
            .driver_state
            .read()
            .map(|s| s.clone())
            .unwrap_or(DriverInstallStatus::Idle);

        if device_info.is_some() || matches!(drv_status, DriverInstallStatus::Running(_)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }

        // Barre supérieure d'état matériel
        egui::TopBottomPanel::top("header_bar")
            .exact_height(56.0)
            .frame(
                Frame::none()
                    .fill(SURFACE_GRAPHITE)
                    .inner_margin(Margin::symmetric(20.0, 0.0))
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.vertical(|ui| {
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("Magic Mouse Studio")
                                .size(16.0)
                                .color(TEXT_PRIMARY)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(
                                "Pilote Multi-Touch & Inertie haute précision pour Windows",
                            )
                            .size(11.5)
                            .color(TEXT_SECONDARY),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let is_installing = matches!(drv_status, DriverInstallStatus::Running(_));
                        let btn_label = if is_installing {
                            "Installation en cours..."
                        } else {
                            "Installer les pilotes"
                        };

                        let install_btn = egui::Button::new(
                            RichText::new(btn_label)
                                .size(11.5)
                                .color(if is_installing {
                                    TEXT_SECONDARY
                                } else {
                                    BG_OBSIDIAN
                                })
                                .strong(),
                        )
                        .fill(if is_installing {
                            SURFACE_ELEVATED
                        } else {
                            ACCENT_OPTICAL
                        })
                        .rounding(Rounding::same(14.0));

                        if ui
                            .add_enabled(!is_installing, install_btn)
                            .on_hover_text(
                                "Installe ou répare les pilotes Boot Camp / HID de la Magic Mouse et active le Multi-Touch",
                            )
                            .clicked()
                        {
                            spawn_driver_installation(self.driver_state.clone());
                        }

                        ui.add_space(6.0);

                        let (dot_color, label_text) = match &device_info {
                            Some(info) if info.multitouch_unlocked => (
                                ACCENT_EMERALD,
                                format!("{} · Multi-Touch actif", info.model.display_name()),
                            ),
                            Some(info) => (
                                ACCENT_AMBER,
                                format!("{} · Mode standard", info.model.display_name()),
                            ),
                            None => (
                                TEXT_MUTED,
                                "Aucune souris détectée · Veille USB / Bluetooth".to_string(),
                            ),
                        };

                        Frame::none()
                            .fill(BG_OBSIDIAN)
                            .rounding(Rounding::same(14.0))
                            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                            .inner_margin(Margin::symmetric(12.0, 6.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let (dot_rect, _) =
                                        ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                                    ui.painter().circle_filled(
                                        dot_rect.center(),
                                        4.0,
                                        dot_color,
                                    );
                                    ui.label(
                                        RichText::new(label_text)
                                            .size(11.5)
                                            .color(TEXT_PRIMARY),
                                    );
                                });
                            });
                    });
                });
            });

        // Barre inférieure discrète
        egui::TopBottomPanel::bottom("footer_bar")
            .exact_height(34.0)
            .frame(
                Frame::none()
                    .fill(BG_OBSIDIAN)
                    .inner_margin(Margin::symmetric(20.0, 0.0))
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new(
                            "Fermer la fenêtre [×] maintient l'app active en arrière-plan (System Tray)",
                        )
                        .size(11.0)
                        .color(TEXT_MUTED),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button(
                                RichText::new("Réduire en arrière-plan")
                                    .size(11.5)
                                    .color(TEXT_SECONDARY),
                            )
                            .clicked()
                        {
                            hide_gui_window_to_tray(ctx);
                        }

                        if ui
                            .button(RichText::new("Réinitialiser").size(11.5).color(TEXT_SECONDARY))
                            .clicked()
                        {
                            let reset_cfg = AppConfig::default();
                            if let Ok(mut cfg) = self.config.write() {
                                *cfg = reset_cfg.clone();
                            }
                            self.save_config(&reset_cfg);
                        }

                        if let Some((msg, ts)) = &self.status_toast {
                            if ts.elapsed().as_secs_f32() < 3.0 {
                                ui.label(
                                    RichText::new(msg)
                                        .size(11.5)
                                        .color(ACCENT_EMERALD),
                                );
                            }
                        }
                    });
                });
            });

        // Corps principal en 2 colonnes : Radar Capacitif à gauche + Réglages & Pilotes à droite
        egui::CentralPanel::default()
            .frame(Frame::none().fill(BG_OBSIDIAN).inner_margin(Margin::same(18.0)))
            .show(ctx, |ui| {
                let mut cfg_snapshot = self
                    .config
                    .read()
                    .map(|c| c.clone())
                    .unwrap_or_default();
                let mut config_modified = false;

                ui.columns(2, |columns| {
                    self.draw_capacitive_surface(&mut columns[0], &cfg_snapshot);

                    let right_ui = &mut columns[1];
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(right_ui, |ui| {
                            // Section 1 : Pilotes & Connexion Matérielle
                            Frame::none()
                                .fill(SURFACE_GRAPHITE)
                                .rounding(Rounding::same(10.0))
                                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                .inner_margin(Margin::same(16.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("Pilotes & Activation Multi-Touch")
                                            .size(14.5)
                                            .color(TEXT_PRIMARY)
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new(
                                            "Installe les pilotes Apple / HID et débloque le flux capacitif de la Magic Mouse.",
                                        )
                                        .size(11.5)
                                        .color(TEXT_SECONDARY),
                                    );
                                    ui.add_space(6.0);

                                    let is_installing =
                                        matches!(drv_status, DriverInstallStatus::Running(_));

                                    ui.horizontal(|ui| {
                                        let primary_btn = egui::Button::new(
                                            RichText::new(if is_installing {
                                                "Installation des pilotes en cours..."
                                            } else {
                                                "Installer / Réparer les pilotes"
                                            })
                                            .size(12.5)
                                            .color(if is_installing {
                                                TEXT_SECONDARY
                                            } else {
                                                BG_OBSIDIAN
                                            })
                                            .strong(),
                                        )
                                        .fill(if is_installing {
                                            SURFACE_ELEVATED
                                        } else {
                                            ACCENT_OPTICAL
                                        })
                                        .rounding(Rounding::same(6.0));

                                        if ui.add_enabled(!is_installing, primary_btn).clicked() {
                                            spawn_driver_installation(self.driver_state.clone());
                                        }

                                        if ui
                                            .button(
                                                RichText::new("Réveiller Multi-Touch (HID)")
                                                    .size(12.0)
                                                    .color(TEXT_PRIMARY),
                                            )
                                            .on_hover_text(
                                                "Envoie immédiatement le Feature Report Apple aux collections HID détectées sans droits admin",
                                            )
                                            .clicked()
                                        {
                                            let (found, unlocked) = force_unlock_all_magic_mice();
                                            let msg = if unlocked > 0 {
                                                format!(
                                                    "Multi-Touch réveillé ({}/{} collections HID)",
                                                    unlocked, found
                                                )
                                            } else if found > 0 {
                                                format!(
                                                    "{} interface(s) trouvée(s), cliquez sur Installer les pilotes",
                                                    found
                                                )
                                            } else {
                                                "Aucune Magic Mouse détectée en USB/Bluetooth"
                                                    .to_string()
                                            };
                                            self.status_toast = Some((msg, Instant::now()));
                                        }
                                    });

                                    match &drv_status {
                                        DriverInstallStatus::Idle => {}
                                        DriverInstallStatus::Running(msg) => {
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(format!("⏳ {msg}"))
                                                    .size(11.5)
                                                    .color(ACCENT_AMBER),
                                            );
                                        }
                                        DriverInstallStatus::Success(msg, _) => {
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(format!("✔ {msg}"))
                                                    .size(11.5)
                                                    .color(ACCENT_EMERALD),
                                            );
                                        }
                                        DriverInstallStatus::Error(msg, _) => {
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(format!("⚠ {msg}"))
                                                    .size(11.5)
                                                    .color(ACCENT_CORAL),
                                            );
                                        }
                                    }
                                });

                            ui.add_space(12.0);

                            // Section 2 : Défilement & Inertie
                            Frame::none()
                                .fill(SURFACE_GRAPHITE)
                                .rounding(Rounding::same(10.0))
                                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                .inner_margin(Margin::same(16.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("Défilement et cinétique")
                                            .size(14.5)
                                            .color(TEXT_PRIMARY)
                                            .strong(),
                                    );
                                    ui.add_space(4.0);

                                    if Self::draw_toggle_row(
                                        ui,
                                        "Défilement naturel",
                                        "Inverse le sens du mouvement comme sur macOS",
                                        &mut cfg_snapshot.natural_scrolling,
                                    ) {
                                        config_modified = true;
                                    }

                                    ui.separator();

                                    if Self::draw_toggle_row(
                                        ui,
                                        "Inertie fluide (125 Hz)",
                                        "Poursuit la glisse après avoir levé le doigt de la coque",
                                        &mut cfg_snapshot.inertia_enabled,
                                    ) {
                                        config_modified = true;
                                    }

                                    ui.separator();

                                    if Self::draw_toggle_row(
                                        ui,
                                        "Défilement horizontal 360°",
                                        "Autorise le déplacement gauche/droite avec verrouillage d'axe",
                                        &mut cfg_snapshot.horizontal_scroll_enabled,
                                    ) {
                                        config_modified = true;
                                    }

                                    ui.add_space(6.0);

                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("Vitesse verticale")
                                                .size(12.5)
                                                .color(TEXT_SECONDARY),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .add(
                                                        egui::Slider::new(
                                                            &mut cfg_snapshot.scroll_speed_vertical,
                                                            0.4..=3.0,
                                                        )
                                                        .suffix("x")
                                                        .step_by(0.05),
                                                    )
                                                    .changed()
                                                {
                                                    config_modified = true;
                                                }
                                            },
                                        );
                                    });

                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("Vitesse horizontale")
                                                .size(12.5)
                                                .color(TEXT_SECONDARY),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .add_enabled(
                                                        cfg_snapshot.horizontal_scroll_enabled,
                                                        egui::Slider::new(
                                                            &mut cfg_snapshot
                                                                .scroll_speed_horizontal,
                                                            0.4..=3.0,
                                                        )
                                                        .suffix("x")
                                                        .step_by(0.05),
                                                    )
                                                    .changed()
                                                {
                                                    config_modified = true;
                                                }
                                            },
                                        );
                                    });

                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("Longueur de glisse (Inertie)")
                                                .size(12.5)
                                                .color(TEXT_SECONDARY),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .add_enabled(
                                                        cfg_snapshot.inertia_enabled,
                                                        egui::Slider::new(
                                                            &mut cfg_snapshot.inertia_decay,
                                                            0.80..=0.97,
                                                        )
                                                        .step_by(0.01),
                                                    )
                                                    .changed()
                                                {
                                                    config_modified = true;
                                                }
                                            },
                                        );
                                    });
                                });

                            ui.add_space(12.0);

                            // Section 3 : Zones de Clic Tactile
                            Frame::none()
                                .fill(SURFACE_GRAPHITE)
                                .rounding(Rounding::same(10.0))
                                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                .inner_margin(Margin::same(16.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("Zones de clic tactile")
                                            .size(14.5)
                                            .color(TEXT_PRIMARY)
                                            .strong(),
                                    );
                                    ui.add_space(4.0);

                                    if Self::draw_toggle_row(
                                        ui,
                                        "Clic milieu à 3 doigts",
                                        "Appuyer physiquement avec 3 doigts posés déclenche un clic milieu",
                                        &mut cfg_snapshot.multi_finger_middle_click,
                                    ) {
                                        config_modified = true;
                                    }

                                    ui.separator();

                                    if Self::draw_toggle_row(
                                        ui,
                                        "Bande centrale Clic milieu (1 doigt)",
                                        "Définit un couloir central sur la coque pour le clic milieu",
                                        &mut cfg_snapshot.middle_click_zone_enabled,
                                    ) {
                                        config_modified = true;
                                    }

                                    ui.add_space(6.0);

                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("Frontière du clic droit (X)")
                                                .size(12.5)
                                                .color(TEXT_SECONDARY),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .add(
                                                        egui::Slider::new(
                                                            &mut cfg_snapshot
                                                                .right_click_zone_threshold_x,
                                                            0..=650,
                                                        )
                                                        .suffix(" u"),
                                                    )
                                                    .changed()
                                                {
                                                    config_modified = true;
                                                }
                                            },
                                        );
                                    });

                                    if cfg_snapshot.middle_click_zone_enabled {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new("Largeur de la zone centrale")
                                                    .size(12.5)
                                                    .color(TEXT_SECONDARY),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if ui
                                                        .add(
                                                            egui::Slider::new(
                                                                &mut cfg_snapshot
                                                                    .middle_click_zone_half_width,
                                                                60..=320,
                                                            )
                                                            .suffix(" u"),
                                                        )
                                                        .changed()
                                                    {
                                                        config_modified = true;
                                                    }
                                                },
                                            );
                                        });
                                    }
                                });
                        });
                });

                if config_modified {
                    if let Ok(mut cfg) = self.config.write() {
                        *cfg = cfg_snapshot.clone();
                    }
                    self.save_config(&cfg_snapshot);
                }
            });
    }
}

/// Lance la fenêtre graphique `eframe` (reste active en arrière-plan à la fermeture via System Tray).
pub fn run_gui(
    running: Arc<AtomicBool>,
    config: SharedConfig,
    status: SharedDeviceStatus,
    telemetry: SharedTelemetry,
    driver_state: SharedDriverState,
    start_hidden: bool,
) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Magic Mouse Studio")
            .with_inner_size([860.0, 620.0])
            .with_min_inner_size([760.0, 520.0])
            .with_visible(!start_hidden),
        ..Default::default()
    };

    eframe::run_native(
        "Magic Mouse Studio",
        options,
        Box::new(move |cc| {
            Ok(Box::new(MagicMouseGuiApp::new(
                cc,
                running,
                config,
                status,
                telemetry,
                driver_state,
                start_hidden,
            )))
        }),
    )
}
