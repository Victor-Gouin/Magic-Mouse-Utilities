# Magic Mouse Utilities for Windows (Rust)

Application système ultra-légère en **Rust** (`windows-rs`) permettant de gérer intégralement les **Apple Magic Mouse 1, 2 (Lightning) et 2/3 (USB-C)** sous Windows sans dépendance lourde :
- **Activation directe de la matrice capacitive Multi-Touch** via Feature Reports HID (`[0xF1, 0x02, 0x01]` pour Magic Mouse 2 Lightning/USB-C, `[0xD7, 0x01]` pour Magic Mouse 1).
- **Défilement fluide 360° (Vertical & Horizontal)** avec accumulation sous-pixel (`WHEEL_DELTA` haute résolution) et verrouillage d'axe doux.
- **Inertie cinétique (Momentum Scrolling)** cadencée à 125 Hz via I/O asynchrone (`FILE_FLAG_OVERLAPPED`).
- **Clic Droit tactile & Clic Milieu intelligent** (à 3 doigts posés ou sur la bande centrale à 1 doigt).
- **Icône System Tray native** (`Shell_NotifyIconW`) pour activer/désactiver le défilement naturel, l'inertie, le clic milieu et la vitesse en temps réel.

---

## Architecture du projet

- [`Cargo.toml`](Cargo.toml) : Configuration Cargo et dépendances Win32 (`windows` v0.58).
- [`src/main.rs`](src/main.rs) : Point d'entrée, mode diagnostic (`--diagnose`) et orchestration des threads.
- [`src/protocol.rs`](src/protocol.rs) : Décodage binaire des rapports HID multi-touch Apple (`0x12` pour MM2/USB-C, `0x29` pour MM1) et extraction des coordonnées 12/13 bits signées (`tdata[0..8]`).
- [`src/hid_device.rs`](src/hid_device.rs) : Énumération SetupAPI (`SetupDiGetClassDevsW`), déverrouillage Multi-Touch (`HidD_SetFeature`) et lecture non-bloquante (`ReadFile` + `OVERLAPPED`).
- [`src/gestures.rs`](src/gestures.rs) : Moteur d'interprétation des gestes, suivi des doigts par identifiant de slot (`0..15`), calcul de vélocité et inertie, détection des zones de clic.
- [`src/injector.rs`](src/injector.rs) : Injection bas niveau via `SendInput` (`MOUSEEVENTF_WHEEL`, `MOUSEEVENTF_HWHEEL`, `MOUSEEVENTF_MIDDLEDOWN`, `MOUSEEVENTF_RIGHTDOWN`).
- [`src/tray.rs`](src/tray.rs) : Menu contextuel dans la barre des tâches Windows et sauvegarde automatique dans `magic-mouse-config.json`.
- [`scripts/Install-BootCampMouseDriver.ps1`](scripts/Install-BootCampMouseDriver.ps1) : Script PowerShell de diagnostic PnP et d'installation du pilote Boot Camp (`AppleWirelessMouse64.sys`).

---

## Prérequis : Installer Rust sur Windows

Aucune chaîne de compilation Rust (`cargo` / `rustc`) n'étant installée par défaut sur la machine, exécutez dans PowerShell :

```powershell
winget install Rustlang.Rustup
```
*(Assurez-vous également d'avoir les **Desktop development with C++ / Build Tools for Visual Studio** installés pour l'éditeur de liens `link.exe`).*

---

## Compilation et Exécution

1. **Vérifier la détection de votre Magic Mouse** :
   ```powershell
   cargo run -- --diagnose
   ```
2. **Compiler et lancer en mode Release (arrière-plan System Tray sans console)** :
   ```powershell
   cargo run --release
   ```
   Une icône apparaît dans la zone de notification Windows (System Tray) permettant de personnaliser le défilement et les clics.
