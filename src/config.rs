use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Inversion du défilement (style macOS "Naturel" vs Windows classique)
    pub natural_scrolling: bool,
    /// Activer l'inertie cinétique après avoir relâché le doigt
    pub inertia_enabled: bool,
    /// Facteur de vitesse de défilement vertical (1.0 par défaut)
    pub scroll_speed_vertical: f32,
    /// Facteur de vitesse de défilement horizontal (1.0 par défaut)
    pub scroll_speed_horizontal: f32,
    /// Activer le défilement horizontal (gauche/droite)
    pub horizontal_scroll_enabled: bool,
    /// Activer la détection du clic milieu (zone centrale ou clic à 3 doigts)
    pub middle_click_enabled: bool,
    /// Seuil X (en coordonnées capteur -1100..+1100) à partir duquel un clic physique devient un clic droit
    /// Sur Magic Mouse, X va d'environ -1100 (gauche) à +1100 (droite). 250 = moitié droite franche.
    pub right_click_zone_threshold_x: i32,
    /// Bande centrale [ -middle_zone_half_width .. +middle_zone_half_width ] pour le clic milieu à 1 doigt
    pub middle_click_zone_enabled: bool,
    pub middle_click_zone_half_width: i32,
    /// Clic milieu en appuyant avec 2 ou 3 doigts posés sur la surface
    pub multi_finger_middle_click: bool,
    /// Coefficient de friction de l'inertie (0.90 = arrêt rapide, 0.95 = glisse longue)
    pub inertia_decay: f32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            natural_scrolling: false,
            inertia_enabled: true,
            scroll_speed_vertical: 1.35,
            scroll_speed_horizontal: 1.15,
            horizontal_scroll_enabled: true,
            middle_click_enabled: true,
            right_click_zone_threshold_x: 220,
            middle_click_zone_enabled: false,
            middle_click_zone_half_width: 150,
            multi_finger_middle_click: true,
            inertia_decay: 0.92,
        }
    }
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        let mut path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
        path.pop();
        path.push("magic-mouse-config.json");
        path
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&content) {
                return cfg;
            }
        }
        let cfg = Self::default();
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)
    }
}

pub type SharedConfig = Arc<RwLock<AppConfig>>;
