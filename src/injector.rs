//! Injection d'événements souris natifs Windows via `SendInput` (Win32 API).
//! Gère le défilement haute résolution (sous-pixel / WHEEL_DELTA fractionné),
//! le clic droit tactile et le clic milieu.

use std::mem::size_of;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
    MOUSEINPUT,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulatedButton {
    None,
    Right,
    Middle,
}

/// Injecte un défilement vertical et/ou horizontal dans Windows via `SendInput`.
/// `dy` : positif = défilement vers le haut (WHEEL_DELTA > 0), négatif = vers le bas.
/// `dx` : positif = défilement vers la droite, négatif = vers la gauche.
pub fn inject_scroll(dx: i32, dy: i32) {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(2);

    if dy != 0 {
        inputs.push(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: dy as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }

    if dx != 0 {
        inputs.push(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: dx as u32,
                    dwFlags: MOUSEEVENTF_HWHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }

    if !inputs.is_empty() {
        unsafe {
            let _ = SendInput(&inputs, size_of::<INPUT>() as i32);
        }
    }
}

/// Injecte l'appui ou le relâchement d'un bouton émulé (Clic Droit ou Clic Milieu).
pub fn inject_button(button: EmulatedButton, is_down: bool) {
    let flags = match (button, is_down) {
        (EmulatedButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
        (EmulatedButton::Right, false) => MOUSEEVENTF_RIGHTUP,
        (EmulatedButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
        (EmulatedButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
        (EmulatedButton::None, _) => return,
    };

    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    unsafe {
        let _ = SendInput(&[input], size_of::<INPUT>() as i32);
    }
}
