//! Décodage binaire des rapports HID Multi-Touch d'Apple (Magic Mouse 1, 2 & USB-C)
//! Basé sur le protocole matériel Apple HID (`hid-magicmouse`).

/// Identifiants USB / Bluetooth constructeurs Apple
pub const VID_APPLE_USB: u16 = 0x05AC;
pub const VID_APPLE_BT: u16 = 0x004C;

/// Product IDs des différentes générations de Magic Mouse
pub const PID_MAGIC_MOUSE_1: u16 = 0x030D;
pub const PID_MAGIC_MOUSE_1_ALT: u16 = 0x0310;
pub const PID_MAGIC_MOUSE_2_LIGHTNING: u16 = 0x0269;
pub const PID_MAGIC_MOUSE_2_USBC: u16 = 0x0323;

/// Report IDs utilisés par le firmware de la Magic Mouse
pub const MOUSE_REPORT_ID: u8 = 0x29;   // Magic Mouse 1
pub const MOUSE2_REPORT_ID: u8 = 0x12;  // Magic Mouse 2 (Lightning & USB-C)
pub const BATTERY_REPORT_ID: u8 = 0x30; // Rapport de batterie éventuel

/// Masques d'état d'un point de contact capacitif (octet `tdata[7]`)
pub const TOUCH_STATE_MASK: u8 = 0xF0;
pub const TOUCH_STATE_NONE: u8 = 0x00;
pub const TOUCH_STATE_START: u8 = 0x30;
pub const TOUCH_STATE_DRAG: u8 = 0x40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagicMouseModel {
    MagicMouse1,
    MagicMouse2Lightning,
    MagicMouse2UsbC,
}

impl MagicMouseModel {
    pub fn from_vid_pid(vid: u16, pid: u16) -> Option<Self> {
        if vid != VID_APPLE_USB && vid != VID_APPLE_BT {
            return None;
        }
        match pid {
            PID_MAGIC_MOUSE_1 | PID_MAGIC_MOUSE_1_ALT => Some(Self::MagicMouse1),
            PID_MAGIC_MOUSE_2_LIGHTNING => Some(Self::MagicMouse2Lightning),
            PID_MAGIC_MOUSE_2_USBC => Some(Self::MagicMouse2UsbC),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::MagicMouse1 => "Magic Mouse 1 (0x030D)",
            Self::MagicMouse2Lightning => "Magic Mouse 2 Lightning (0x0269)",
            Self::MagicMouse2UsbC => "Magic Mouse 2 USB-C (0x0323)",
        }
    }

    /// Séquence Feature Report à envoyer via `HidD_SetFeature` pour déverrouiller
    /// l'envoi en continu de la matrice capacitive multi-touch.
    pub fn multitouch_feature_report(&self) -> &'static [u8] {
        match self {
            Self::MagicMouse1 => &[0xD7, 0x01],
            Self::MagicMouse2Lightning | Self::MagicMouse2UsbC => &[0xF1, 0x02, 0x01],
        }
    }
}

/// État d'un doigt individuel détecté sur la surface capacitive de la souris
#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    /// Identifiant de suivi du doigt (0..15)
    pub id: u8,
    /// Coordonnée X signée sur la surface (env. -1100 à gauche .. +1100 à droite)
    pub x: i32,
    /// Coordonnée Y signée sur la surface (env. -1500 en bas .. +1500 en haut)
    pub y: i32,
    /// Grand axe de l'ellipse de contact
    pub touch_major: u8,
    /// Petit axe de l'ellipse de contact
    pub touch_minor: u8,
    /// Taille / pression relative (6 bits : 0..63)
    pub size: u8,
    /// Orientation du doigt (-32..+31)
    pub orientation: i8,
    /// État brut du contact (`TOUCH_STATE_START`, `TOUCH_STATE_DRAG`, etc.)
    pub state: u8,
    /// Indique si le doigt est effectivement posé sur la surface
    pub is_down: bool,
}

/// Rapport complet décodé depuis un paquet HID de la Magic Mouse
#[derive(Debug, Clone)]
pub struct MagicMouseReport {
    pub report_id: u8,
    /// Déplacement relatif optique X de la souris (si présent dans ce rapport)
    pub rel_x: i32,
    /// Déplacement relatif optique Y de la souris (si présent dans ce rapport)
    pub rel_y: i32,
    /// État du bouton physique sous la coque (bit 0 = pressé)
    pub physical_click: bool,
    /// Liste des points de contact capacitifs détectés dans cette trame
    pub touches: Vec<TouchPoint>,
}

/// Décode un bloc de 8 octets (`tdata[0..8]`) représentant un doigt pour Magic Mouse 1 ou 2.
pub fn parse_touch_point(model: MagicMouseModel, tdata: &[u8]) -> Option<TouchPoint> {
    if tdata.len() < 8 {
        return None;
    }

    let id = (( (tdata[6] as u16) << 2 ) | ( (tdata[5] as u16) >> 6 )) as u8 & 0x0F;

    let (x, y) = match model {
        MagicMouseModel::MagicMouse1 => {
            // Magic Mouse 1 : coordonnées sur 13 bits signés
            let raw_x = (((tdata[1] as i32) << 27) | ((tdata[0] as i32) << 19)) >> 19;
            let raw_y = -((((tdata[3] as i32) << 30)
                | ((tdata[2] as i32) << 22)
                | ((tdata[1] as i32) << 14))
                >> 19);
            (raw_x, raw_y)
        }
        MagicMouseModel::MagicMouse2Lightning | MagicMouseModel::MagicMouse2UsbC => {
            // Magic Mouse 2 & USB-C : coordonnées sur 12 bits signés
            let raw_x = (((tdata[1] as i32) << 28) | ((tdata[0] as i32) << 20)) >> 20;
            let raw_y = -((((tdata[2] as i32) << 24) | ((tdata[1] as i32) << 16)) >> 20);
            (raw_x, raw_y)
        }
    };

    let touch_major = tdata[3];
    let touch_minor = tdata[4];
    let size = tdata[5] & 0x3F;
    let orientation = ((tdata[6] >> 2) as i8).wrapping_sub(32);
    let state = tdata[7] & TOUCH_STATE_MASK;
    let is_down = state != TOUCH_STATE_NONE;

    Some(TouchPoint {
        id,
        x,
        y,
        touch_major,
        touch_minor,
        size,
        orientation,
        state,
        is_down,
    })
}

/// Analyse un paquet HID brut reçu depuis `ReadFile` sur la collection HID de la Magic Mouse.
pub fn parse_hid_report(model: MagicMouseModel, data: &[u8]) -> Option<MagicMouseReport> {
    if data.is_empty() {
        return None;
    }

    let report_id = data[0];
    let size = data.len();

    match report_id {
        MOUSE2_REPORT_ID => {
            // Magic Mouse 2 : la taille est soit 8 octets, soit 14 + N * 8 octets
            if size != 8 && (size < 14 || (size - 14) % 8 != 0) {
                return None;
            }

            let npoints = if size >= 14 { (size - 14) / 8 } else { 0 };
            if npoints > 15 {
                return None;
            }

            let mut touches = Vec::with_capacity(npoints);
            for i in 0..npoints {
                let offset = 14 + i * 8;
                if let Some(tp) = parse_touch_point(model, &data[offset..offset + 8]) {
                    touches.push(tp);
                }
            }

            let clicks = data[1];
            let rel_x = i16::from_le_bytes([data[2], data[3]]) as i32;
            let rel_y = i16::from_le_bytes([data[4], data[5]]) as i32;

            Some(MagicMouseReport {
                report_id,
                rel_x,
                rel_y,
                physical_click: (clicks & 0x03) != 0,
                touches,
            })
        }
        MOUSE_REPORT_ID => {
            // Magic Mouse 1 : 6 octets d'en-tête + N * 8 octets de points tactiles
            if size < 6 || (size - 6) % 8 != 0 {
                return None;
            }

            let npoints = (size - 6) / 8;
            if npoints > 15 {
                return None;
            }

            let mut touches = Vec::with_capacity(npoints);
            for i in 0..npoints {
                let offset = 6 + i * 8;
                if let Some(tp) = parse_touch_point(MagicMouseModel::MagicMouse1, &data[offset..offset + 8]) {
                    touches.push(tp);
                }
            }

            let rel_x = ((((data[3] as i32) & 0x0C) << 28) | ((data[1] as i32) << 22)) >> 22;
            let rel_y = ((((data[3] as i32) & 0x30) << 26) | ((data[2] as i32) << 22)) >> 22;
            let clicks = data[3];

            Some(MagicMouseReport {
                report_id,
                rel_x,
                rel_y,
                physical_click: (clicks & 0x03) != 0,
                touches,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_magic_mouse2_report_with_one_finger() {
        // Simule un rapport 0x12 de Magic Mouse 2 avec 1 doigt (14 + 8 = 22 octets)
        let mut packet = vec![0u8; 22];
        packet[0] = MOUSE2_REPORT_ID;
        packet[1] = 0x01; // Clic physique pressé

        // Point de contact à l'offset 14..22
        // x = +300 (0x12C), y = -200
        // tdata[0] = 0x2C, tdata[1] bas = 0x01
        packet[14] = 0x2C;
        packet[15] = 0x01;
        packet[16] = 0x05;
        packet[17] = 20; // major
        packet[18] = 15; // minor
        packet[19] = (1 << 6) | 12; // id low bits + size 12
        packet[20] = 0x80;
        packet[21] = TOUCH_STATE_DRAG;

        let report = parse_hid_report(MagicMouseModel::MagicMouse2Lightning, &packet)
            .expect("Devrait décoder le rapport MM2");
        assert!(report.physical_click);
        assert_eq!(report.touches.len(), 1);
        let tp = &report.touches[0];
        assert_eq!(tp.x, 0x12C);
        assert!(tp.is_down);
    }
}
