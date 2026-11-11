# Magic Mouse Utilities for Windows (`magic-mouse-win`) — Project Memory & Technical Guide

Ce document contient toutes les informations d'architecture, de protocole HID Apple, de configuration de compilation Rust sur cette machine et de design UI à respecter lors des futurs prompts.

---

## 1. Vue d'ensemble du Projet
- **Nom du crate / binaire** : `magic-mouse-win` (`target/release/magic-mouse-win.exe`)
- **Langage & Stack** : Rust (Edition 2021)
  - APIs système bas niveau : `windows = "0.58"` (SetupAPI, HidD_*, FileSystem Overlapped I/O, SendInput, Shell_NotifyIconW).
  - Interface graphique native : `eframe = "0.29"` + `egui = "0.29"` (backend OpenGL `glow`).
  - Sérialisation : `serde` + `serde_json` (`magic-mouse-config.json` stocké à côté de l'exécutable).

---

## 2. Contraintes de Compilation Rust sur cette Machine (IMPORTANT)
La machine Windows utilise la chaîne autonome **`stable-x86_64-pc-windows-gnu`** (car Visual Studio C++ `link.exe` et MSYS2 complet ne sont pas installés).
Pour que `cargo check`, `cargo test` et `cargo build --release` fonctionnent sans erreur d'éditeur de liens :

1. **Ajout de Cargo au `PATH` dans chaque terminal PowerShell** :
   ```powershell
   $env:PATH += ";$env:USERPROFILE\.cargo\bin"
   ```
2. **Interdiction des crates déclenchant `dlltool.exe` via `raw-dylib` (`windows-link`)** :
   - Le binaire `self-contained\dlltool.exe` de Rust GNU échoue s'il est appelé car l'assembleur externe `as.exe` n'est pas installé.
   - Dans `Cargo.lock`, conserver les versions suivantes qui utilisent les bibliothèques d'importation pré-compilées `windows_x86_64_gnu` :
     - `parking_lot = "0.12.3"` et `parking_lot_core = "0.9.10"`
     - `libloading = "0.8.6"`
     - `arboard = "3.4.1"`
     - `webbrowser = "1.0.3"` (avec `windows-sys = "0.45.0"`)
   - **Ne pas lancer `cargo update` global** sans maintenir ces versions épinglées.
3. **Rôle de [`build.rs`](build.rs)** :
   - Copie automatiquement `libwinapi_shlwapi.a` (fourni par `winapi-x86_64-pc-windows-gnu-0.4.0`) vers `OUT_DIR/libshlwapi.a` pour que le linker GNU autonome résolve `-lshlwapi` demandé par `webbrowser`.

---

## 3. Protocole Matériel Apple Magic Mouse ([`src/protocol.rs`](src/protocol.rs))

### Identifiants Matériels (VID / PID)
- **Vendor IDs Apple** :
  - USB : `0x05AC` (`VID_APPLE_USB`)
  - Bluetooth : `0x004C` (`VID_APPLE_BT`)
- **Product IDs supportés** :
  - **Magic Mouse 1 (Piles AA)** : `0x030D` (`PID_MAGIC_MOUSE_1`) et `0x0310` (`PID_MAGIC_MOUSE_1_ALT`)
  - **Magic Mouse 2 (Lightning)** : `0x0269` (`PID_MAGIC_MOUSE_2_LIGHTNING`)
  - **Magic Mouse 2 / 3 (USB-C)** : `0x0323` (`PID_MAGIC_MOUSE_2_USBC`)

### Activation Multi-Touch (`HidD_SetFeature`)
Par défaut sous Windows, la Magic Mouse ne renvoie que le pointage et le clic basique. Pour activer le flux capacitif multi-touch :
- **Magic Mouse 1** : envoyer le Feature Report `[0xD7, 0x01]`
- **Magic Mouse 2 (Lightning & USB-C)** : envoyer le Feature Report `[0xF1, 0x02, 0x01]`

### Structure des Rapports d'Entrée HID (`ReadFile`)
- **Magic Mouse 2 / USB-C (`MOUSE2_REPORT_ID = 0x12`)** :
  - Taille valide : `8` octets (état simple) ou `14 + N * 8` octets (`N` = nombre de doigts actifs, `0..=15`).
  - `data[1]` : état des boutons physiques (`clicks & 0x03`).
  - `data[2..4]` / `data[4..6]` : déplacement optique relatif X/Y (`i16` little-endian).
  - À partir de l'offset `14`, chaque tranche de **8 octets (`tdata[0..8]`)** encode un point de contact :
    - `id` (4 bits) : `((tdata[6] << 2) | (tdata[5] >> 6)) & 0x0F`
    - `x` (12 bits signés, env. `-1100` à `+1100`) : `((tdata[1] as i32) << 28 | (tdata[0] as i32) << 20) >> 20`
    - `y` (12 bits signés inversés, env. `-1500` à `+1500`) : `-(((tdata[2] as i32) << 24 | (tdata[1] as i32) << 16) >> 20)`
    - `touch_major` : `tdata[3]`, `touch_minor` : `tdata[4]`
    - `size` (6 bits) : `tdata[5] & 0x3F`
    - `orientation` : `((tdata[6] >> 2) as i8).wrapping_sub(32)`
    - `state` : `tdata[7] & 0xF0` (`0x00` = relâché, `0x30` = `START`, `0x40` = `DRAG`).
- **Magic Mouse 1 (`MOUSE_REPORT_ID = 0x29`)** :
  - Taille valide : `6 + N * 8` octets.

---

## 4. Architecture des Modules ([`src/`](src/))

- [`src/main.rs`](src/main.rs) :
  - Point d'entrée et gestion des arguments CLI :
    - Sans argument : démarre le worker HID, lance le System Tray en thread secondaire et ouvre la fenêtre graphique **Magic Mouse Studio**.
    - `--tray` : exécute uniquement l'application en arrière-plan dans la barre des tâches Windows (System Tray).
    - `--diagnose` : énumère et affiche toutes les collections HID Apple détectées sur le PC.
    - `--simulate` : synthétise de vraies trames binaires HID `0x12` de Magic Mouse 2 (sans souris physique) et teste le scroll, l'inertie et les clics.
- [`src/hid_device.rs`](src/hid_device.rs) :
  - Énumération SetupAPI (`SetupDiGetClassDevsW`), déverrouillage Multi-Touch (`HidD_SetFeature`), boucle de lecture asynchrone `FILE_FLAG_OVERLAPPED` avec réveil toutes les **8 ms (125 Hz)** (`WaitForSingleObject`) pour cadencer l'inertie cinétique.
  - Met à jour `SharedDeviceStatus` et `SharedTelemetry` (`LiveTelemetry`) en temps réel pour l'interface graphique.
- [`src/gestures.rs`](src/gestures.rs) :
  - `GestureEngine` : suivi des doigts par slot ID, filtrage du bruit au poser (`TOUCH_STATE_START`), verrouillage d'axe doux (vertical vs horizontal), accumulation sous-pixel, vélocité exponentielle et décélération d'inertie.
  - Utilise le trait `InputSink` (`Win32InputSink` en production, `RecordingSink` pour les tests unitaires et `--simulate`).
- [`src/injector.rs`](src/injector.rs) :
  - Injection Win32 native via `SendInput` (`MOUSEEVENTF_WHEEL`, `MOUSEEVENTF_HWHEEL`, `MOUSEEVENTF_RIGHTDOWN/UP`, `MOUSEEVENTF_MIDDLEDOWN/UP`).
- [`src/driver.rs`](src/driver.rs) :
  - Installation et réparation en arrière-plan des pilotes Apple / HID (`spawn_driver_installation` exécutant [`scripts/Install-BootCampMouseDriver.ps1`](scripts/Install-BootCampMouseDriver.ps1) avec élévation UAC si nécessaire, embarqué via `include_str!` pour autonomie complète du `.exe`).
  - Réveil Multi-Touch immédiat (`force_unlock_all_magic_mice`) envoyant le Feature Report à toutes les collections HID Apple détectées.
- [`src/gui.rs`](src/gui.rs) :
  - Fenêtre native **Magic Mouse Studio** (`egui`/`eframe`).
  - Palette sobre « Atelier Aluminium & Graphite » (`#111214` Obsidienne, `#17191C` Graphite, bordures 1px `#26292E`, accents `#38BDF8` et `#10B981`).
  - Intercepte la fermeture `[×]` (`ViewportCommand::CancelClose` + `hide_gui_window_to_tray`) pour maintenir l'application en arrière-plan dans le System Tray.
  - Barre supérieure et panneau droit incluant les boutons **« Installer les pilotes »** et **« Réveiller Multi-Touch (HID) »**.
  - Colonne gauche : **Radar Capacitif** dessinant la Magic Mouse à l'échelle, les zones de clic (Droite / Milieu), les doigts en direct et un **Mode Simulateur Interactif** permettant de tester à la souris sans matériel.
  - Colonne droite : Contrôles temps-réel sauvegardés automatiquement dans `magic-mouse-config.json`.
- [`src/tray.rs`](src/tray.rs) :
  - Icône System Tray Win32 (`Shell_NotifyIconW`) avec notification ballon lors du passage en arrière-plan, restauration instantanée de la fenêtre (`restore_gui_window` par double-clic ou menu), bouton d'installation des pilotes et détection d'instance unique (`focus_existing_instance_if_running`).
- [`scripts/Install-BootCampMouseDriver.ps1`](scripts/Install-BootCampMouseDriver.ps1) :
  - Script PowerShell de vérification PnP et d'installation du pilote Boot Camp (`AppleWirelessMouse64.sys`) si nécessaire.
