//! Moteur d'interprétation des gestes capacitifs de la Magic Mouse :
//! - Suivi multi-doigts (slots 0..15)
//! - Défilement vertical et horizontal haute précision avec accumulation sous-pixel
//! - Inertie cinétique (momentum scrolling) rafraîchie à ~120 Hz
//! - Détection de zone tactile pour le Clic Droit et Clic Milieu (1 doigt au centre ou 2/3 doigts)

use crate::config::AppConfig;
use crate::injector::{inject_button, inject_scroll, EmulatedButton};
use crate::protocol::{MagicMouseReport, TOUCH_STATE_DRAG, TOUCH_STATE_START};
use std::collections::HashMap;
use std::time::Instant;

/// Événement de sortie produit par le moteur de gestes (injecté dans Windows ou observé en simulation/test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureOutput {
    Scroll { dx: i32, dy: i32 },
    Button { button: EmulatedButton, is_down: bool },
}

/// Trait d'abstraction pour découpler le moteur de gestes de l'API Win32 `SendInput` (idéal pour les tests sans matériel).
pub trait InputSink {
    fn on_output(&mut self, output: GestureOutput);
}

/// Implémentation réelle qui appelle directement `SendInput` sous Windows.
pub struct Win32InputSink;

impl InputSink for Win32InputSink {
    fn on_output(&mut self, output: GestureOutput) {
        match output {
            GestureOutput::Scroll { dx, dy } => inject_scroll(dx, dy),
            GestureOutput::Button { button, is_down } => inject_button(button, is_down),
        }
    }
}

/// Implémentation de capture pour les tests unitaires et le mode `--simulate`.
#[derive(Default)]
pub struct RecordingSink {
    pub events: Vec<GestureOutput>,
}

impl InputSink for RecordingSink {
    fn on_output(&mut self, output: GestureOutput) {
        self.events.push(output);
    }
}

#[derive(Debug, Clone, Copy)]
struct TrackedFinger {
    x: i32,
    y: i32,
    start_x: i32,
    start_y: i32,
    last_update: Instant,
    is_scrolling: bool,
}

pub struct GestureEngine {
    fingers: HashMap<u8, TrackedFinger>,
    /// Vitesse instantanée lissée (en unités de scroll par trame)
    vel_x: f32,
    vel_y: f32,
    /// Indique si le défilement inertiel est actuellement en cours (doigts levés)
    in_inertia: bool,
    /// Accumulateurs fractionnaires pour conserver une fluidité sous-pixel
    accum_x: f32,
    accum_y: f32,
    /// État du clic physique précédent
    prev_physical_click: bool,
    /// Si un clic droit ou milieu a été déclenché, on le mémorise jusqu'au relâchement physique
    active_emulated_button: EmulatedButton,
    last_inertia_tick: Instant,
}

impl GestureEngine {
    pub fn new() -> Self {
        Self {
            fingers: HashMap::with_capacity(16),
            vel_x: 0.0,
            vel_y: 0.0,
            in_inertia: false,
            accum_x: 0.0,
            accum_y: 0.0,
            prev_physical_click: false,
            active_emulated_button: EmulatedButton::None,
            last_inertia_tick: Instant::now(),
        }
    }

    pub fn is_in_inertia(&self) -> bool {
        self.in_inertia
    }

    pub fn velocity(&self) -> (f32, f32) {
        (self.vel_x, self.vel_y)
    }

    pub fn active_emulated_button(&self) -> EmulatedButton {
        self.active_emulated_button
    }

    /// Traite un nouveau rapport HID avec injection Win32 directe.
    pub fn process_report(&mut self, report: &MagicMouseReport, cfg: &AppConfig) {
        let mut sink = Win32InputSink;
        self.process_report_with_sink(report, cfg, &mut sink);
    }

    /// Traite un nouveau rapport HID provenant de la Magic Mouse vers un `InputSink` générique.
    pub fn process_report_with_sink<S: InputSink>(
        &mut self,
        report: &MagicMouseReport,
        cfg: &AppConfig,
        sink: &mut S,
    ) {
        let now = Instant::now();

        // 1. Mise à jour des doigts actifs et calcul du déplacement delta pour le scroll
        let mut active_ids = Vec::with_capacity(report.touches.len());
        let mut scroll_dx_total = 0.0f32;
        let mut scroll_dy_total = 0.0f32;
        let mut scrolling_fingers_count = 0;

        // Ne pas scroller si le bouton physique est enfoncé (évite les scrolls parasites pendant un drag)
        let allow_scroll = !report.physical_click;

        for tp in &report.touches {
            if !tp.is_down {
                self.fingers.remove(&tp.id);
                continue;
            }

            active_ids.push(tp.id);

            if let Some(prev) = self.fingers.get_mut(&tp.id) {
                let raw_dx = (tp.x - prev.x) as f32;
                let raw_dy = (tp.y - prev.y) as f32;

                // Seuil d'engagement du scroll pour éviter les micro-tremblements au repos
                let dist_from_start =
                    ((tp.x - prev.start_x).abs() + (tp.y - prev.start_y).abs()) as f32;

                if dist_from_start > 14.0 {
                    prev.is_scrolling = true;
                }

                // Ignorer la toute première trame TOUCH_STATE_START pour éviter les sauts
                if allow_scroll && prev.is_scrolling && tp.state == TOUCH_STATE_DRAG {
                    scroll_dx_total += raw_dx;
                    scroll_dy_total += raw_dy;
                    scrolling_fingers_count += 1;
                }

                prev.x = tp.x;
                prev.y = tp.y;
                prev.last_update = now;
            } else {
                // Nouveau doigt posé sur la surface : coupe immédiatement toute inertie en cours
                self.in_inertia = false;
                self.vel_x = 0.0;
                self.vel_y = 0.0;

                self.fingers.insert(
                    tp.id,
                    TrackedFinger {
                        x: tp.x,
                        y: tp.y,
                        start_x: tp.x,
                        start_y: tp.y,
                        last_update: now,
                        is_scrolling: tp.state != TOUCH_STATE_START,
                    },
                );
            }
        }

        // Nettoyage des doigts qui ne sont plus dans le rapport
        self.fingers.retain(|id, _| active_ids.contains(id));

        // 2. Déclenchement et émission du défilement (1 doigt ou 2 doigts)
        if scrolling_fingers_count > 0 && scrolling_fingers_count <= 2 {
            let avg_dx = scroll_dx_total / (scrolling_fingers_count as f32);
            let avg_dy = scroll_dy_total / (scrolling_fingers_count as f32);

            let sign = if cfg.natural_scrolling { -1.0 } else { 1.0 };

            // Conversion des unités capteur Apple vers des fractions de WHEEL_DELTA (120)
            let scaled_dy = -avg_dy * cfg.scroll_speed_vertical * 0.85 * sign;
            let scaled_dx = if cfg.horizontal_scroll_enabled {
                avg_dx * cfg.scroll_speed_horizontal * 0.75 * sign
            } else {
                0.0
            };

            // Verrouillage d'axe doux : si le mouvement est principalement vertical (> 2.5x),
            // on atténue le défilement horizontal involontaire
            let (final_dx, final_dy) = if scaled_dy.abs() > scaled_dx.abs() * 2.5 {
                (0.0, scaled_dy)
            } else if scaled_dx.abs() > scaled_dy.abs() * 2.5 {
                (scaled_dx, 0.0)
            } else {
                (scaled_dx, scaled_dy)
            };

            self.emit_fractional_scroll(final_dx, final_dy, sink);

            // Mise à jour de la vélocité exponentielle pour l'inertie au lâcher du doigt
            self.vel_x = self.vel_x * 0.45 + final_dx * 0.55;
            self.vel_y = self.vel_y * 0.45 + final_dy * 0.55;
            self.in_inertia = false;
        } else if self.fingers.is_empty() && cfg.inertia_enabled {
            // Tous les doigts viennent d'être levés : démarrer l'inertie si la vitesse est suffisante
            if !self.in_inertia && (self.vel_x.abs() > 1.8 || self.vel_y.abs() > 1.8) {
                self.in_inertia = true;
                self.last_inertia_tick = now;
            }
        }

        // 3. Gestion intelligente des clics (Clic Droit / Clic Milieu selon la position des doigts)
        self.handle_clicks(report.physical_click, cfg, sink);
    }

    /// Avance l'inertie cinétique avec injection Win32 directe.
    pub fn tick_inertia(&mut self, cfg: &AppConfig) {
        let mut sink = Win32InputSink;
        self.tick_inertia_with_sink(cfg, &mut sink, false);
    }

    /// Appelé périodiquement (ex: toutes les 8 ms quand la souris n'envoie pas de paquet)
    /// pour faire avancer le défilement cinétique (inertie) de manière ultra-fluide.
    pub fn tick_inertia_with_sink<S: InputSink>(
        &mut self,
        cfg: &AppConfig,
        sink: &mut S,
        ignore_timer: bool,
    ) {
        if !self.in_inertia || !cfg.inertia_enabled {
            return;
        }

        if !ignore_timer {
            let now = Instant::now();
            let elapsed_ms = now.duration_since(self.last_inertia_tick).as_secs_f32() * 1000.0;
            if elapsed_ms < 7.0 {
                return;
            }
            self.last_inertia_tick = now;
        }

        let decay = cfg.inertia_decay.clamp(0.75, 0.98);
        self.vel_x *= decay;
        self.vel_y *= decay;

        if self.vel_x.abs() < 0.4 && self.vel_y.abs() < 0.4 {
            self.in_inertia = false;
            self.vel_x = 0.0;
            self.vel_y = 0.0;
            return;
        }

        self.emit_fractional_scroll(self.vel_x, self.vel_y, sink);
    }

    fn emit_fractional_scroll<S: InputSink>(&mut self, dx: f32, dy: f32, sink: &mut S) {
        self.accum_x += dx;
        self.accum_y += dy;

        let send_x = self.accum_x.trunc() as i32;
        let send_y = self.accum_y.trunc() as i32;

        if send_x != 0 || send_y != 0 {
            self.accum_x -= send_x as f32;
            self.accum_y -= send_y as f32;
            sink.on_output(GestureOutput::Scroll {
                dx: send_x,
                dy: send_y,
            });
        }
    }

    fn handle_clicks<S: InputSink>(&mut self, physical_click: bool, cfg: &AppConfig, sink: &mut S) {
        if physical_click && !self.prev_physical_click {
            // Front montant du clic physique : déterminer quel bouton émulé activer
            let button = self.classify_click_zone(cfg);
            if button != EmulatedButton::None {
                self.active_emulated_button = button;
                sink.on_output(GestureOutput::Button {
                    button,
                    is_down: true,
                });
            }
        } else if !physical_click && self.prev_physical_click {
            // Front descendant : relâcher le bouton émulé s'il était actif
            if self.active_emulated_button != EmulatedButton::None {
                sink.on_output(GestureOutput::Button {
                    button: self.active_emulated_button,
                    is_down: false,
                });
                self.active_emulated_button = EmulatedButton::None;
            }
        }

        self.prev_physical_click = physical_click;
    }

    fn classify_click_zone(&self, cfg: &AppConfig) -> EmulatedButton {
        if self.fingers.is_empty() {
            return EmulatedButton::None;
        }

        // Option 1 : Clic Milieu à 3 doigts (ou 2 doigts espacés si configuré)
        if cfg.middle_click_enabled && cfg.multi_finger_middle_click && self.fingers.len() >= 3 {
            return EmulatedButton::Middle;
        }

        // On prend le doigt le plus haut sur la coque (plus grand Y) pour déterminer la zone de clic
        if let Some(primary) = self.fingers.values().max_by_key(|f| f.y) {
            // Zone centrale pour le Clic Milieu à 1 doigt (si activée)
            if cfg.middle_click_enabled
                && cfg.middle_click_zone_enabled
                && primary.x.abs() <= cfg.middle_click_zone_half_width
            {
                return EmulatedButton::Middle;
            }

            // Zone droite pour le Clic Droit tactile (si le pilote natif ne l'a pas déjà traduit)
            if primary.x >= cfg.right_click_zone_threshold_x {
                return EmulatedButton::Right;
            }
        }

        EmulatedButton::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{TouchPoint, MOUSE2_REPORT_ID, TOUCH_STATE_NONE};

    fn make_touch(id: u8, x: i32, y: i32, state: u8) -> TouchPoint {
        TouchPoint {
            id,
            x,
            y,
            touch_major: 20,
            touch_minor: 15,
            size: 10,
            orientation: 0,
            state,
            is_down: state != TOUCH_STATE_NONE,
        }
    }

    #[test]
    fn test_vertical_scroll_and_kinetic_inertia() {
        let mut engine = GestureEngine::new();
        let cfg = AppConfig::default();
        let mut sink = RecordingSink::default();

        // 1. Doigt posé à (0, 0)
        engine.process_report_with_sink(
            &MagicMouseReport {
                report_id: MOUSE2_REPORT_ID,
                rel_x: 0,
                rel_y: 0,
                physical_click: false,
                touches: vec![make_touch(1, 0, 0, TOUCH_STATE_START)],
            },
            &cfg,
            &mut sink,
        );

        // 2. Glissement rapide vers le haut : y passe de 0 -> 30 -> 70 -> 120
        for &y in &[30, 70, 120] {
            engine.process_report_with_sink(
                &MagicMouseReport {
                    report_id: MOUSE2_REPORT_ID,
                    rel_x: 0,
                    rel_y: 0,
                    physical_click: false,
                    touches: vec![make_touch(1, 0, y, TOUCH_STATE_DRAG)],
                },
                &cfg,
                &mut sink,
            );
        }

        let direct_scroll_events = sink.events.len();
        assert!(direct_scroll_events >= 2, "Le glissement doit émettre des événements Scroll");

        // 3. Lever du doigt -> l'inertie doit s'enclencher
        engine.process_report_with_sink(
            &MagicMouseReport {
                report_id: MOUSE2_REPORT_ID,
                rel_x: 0,
                rel_y: 0,
                physical_click: false,
                touches: vec![make_touch(1, 0, 120, TOUCH_STATE_NONE)],
            },
            &cfg,
            &mut sink,
        );

        assert!(engine.is_in_inertia(), "L'inertie cinétique doit être active après un swipe rapide");

        // 4. Exécuter 20 ticks d'inertie
        for _ in 0..20 {
            engine.tick_inertia_with_sink(&cfg, &mut sink, true);
        }

        assert!(
            sink.events.len() > direct_scroll_events,
            "L'inertie doit continuer à émettre des événements de défilement fluide"
        );
    }

    #[test]
    fn test_right_click_and_three_finger_middle_click() {
        let mut engine = GestureEngine::new();
        let cfg = AppConfig::default();
        let mut sink = RecordingSink::default();

        // Clic avec 1 doigt posé sur la moitié droite (X = +450 > seuil 220)
        engine.process_report_with_sink(
            &MagicMouseReport {
                report_id: MOUSE2_REPORT_ID,
                rel_x: 0,
                rel_y: 0,
                physical_click: true,
                touches: vec![make_touch(1, 450, 100, TOUCH_STATE_DRAG)],
            },
            &cfg,
            &mut sink,
        );
        engine.process_report_with_sink(
            &MagicMouseReport {
                report_id: MOUSE2_REPORT_ID,
                rel_x: 0,
                rel_y: 0,
                physical_click: false,
                touches: vec![make_touch(1, 450, 100, TOUCH_STATE_DRAG)],
            },
            &cfg,
            &mut sink,
        );

        assert_eq!(
            sink.events,
            vec![
                GestureOutput::Button {
                    button: EmulatedButton::Right,
                    is_down: true
                },
                GestureOutput::Button {
                    button: EmulatedButton::Right,
                    is_down: false
                },
            ]
        );

        sink.events.clear();

        // Clic avec 3 doigts posés -> Clic Milieu
        engine.process_report_with_sink(
            &MagicMouseReport {
                report_id: MOUSE2_REPORT_ID,
                rel_x: 0,
                rel_y: 0,
                physical_click: true,
                touches: vec![
                    make_touch(1, -300, 100, TOUCH_STATE_DRAG),
                    make_touch(2, 0, 150, TOUCH_STATE_DRAG),
                    make_touch(3, 300, 100, TOUCH_STATE_DRAG),
                ],
            },
            &cfg,
            &mut sink,
        );
        engine.process_report_with_sink(
            &MagicMouseReport {
                report_id: MOUSE2_REPORT_ID,
                rel_x: 0,
                rel_y: 0,
                physical_click: false,
                touches: vec![],
            },
            &cfg,
            &mut sink,
        );

        assert_eq!(
            sink.events,
            vec![
                GestureOutput::Button {
                    button: EmulatedButton::Middle,
                    is_down: true
                },
                GestureOutput::Button {
                    button: EmulatedButton::Middle,
                    is_down: false
                },
            ]
        );
    }
}
