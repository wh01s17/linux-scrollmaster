# Changelog

All notable changes to Linux ScrollMaster are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.0] - 2026-10-27

### Added

- **Current profile** automatically saves each confirmed change to DPI, wheels,
  buttons, shortcuts and gestures. **Save as…** keeps a named copy without
  overwriting existing profiles or querying the mouse again.

### Changed

- Saved settings load automatically when the app opens and when the mouse
  reconnects, without a reapply checkbox. Existing active profiles are preserved;
  otherwise startup prefers Current profile, then the last saved profile for
  that mouse. Keep the app open or in the tray for recovery and software actions.
- Custom shortcuts and gestures apply automatically after a short editing pause;
  exact DPI applies when confirming the field. Separate Apply buttons are removed.

### Fixed

- Consecutive edits are serialized and retain their device identity. Pending
  shortcut and gesture edits survive leaving the editor; choosing the original
  button function cancels its unfinished shortcut.
- Saving reuses the reading that confirms the hardware change. Native polling
  avoids repeating preflight and application when the fresh reading already
  matches the profile.
- Incomplete readings, partial writes and disk failures cannot be reported as a
  successfully saved current profile or silently overwrite a named copy.

### Validation

- 842 automated tests passed, along with the frontend build and Rust check.
  Physical acceptance of the new automatic save/load flow is pending.

## [0.2.1] - 2026-10-26

### Fixed

- Directional gestures could require repeated presses because periodic device
  reads cancelled physical-distance gestures even when DPI had not changed.
  Gestures now survive unchanged reads, including presses received during a
  query, while DPI changes and failed reads still discard unconfirmed actions.

## [0.2.0] - 2026-10-24

### Added

- Scroll distance on Hyprland: a slider and exact factor (0.1–5) in both wheel tabs,
  applied per device to the MX Master through Hyprland IPC and confirmed by reading it
  back. It never edits Hyprland files, is reapplied after a config reload, and
  **Reset** returns to the session's global value. Hidden on other desktops.

## [0.1.2] - 2026-10-24

### Fixed

- With "keep running in the tray" enabled, closing the window opened by autostart
  quit the app instead of hiding it. At login the app could start before the panel's
  tray host, so creating the icon failed and the preference was ignored until the
  next launch. The app now keeps the preference, retries the icon for two minutes
  and once more on close.

## [0.1.1] - 2026-10-24

### Fixed

- Autostart no longer registers a development build (`pnpm tauri dev`). Such a build
  loads its interface from the Vite dev server, so after a reboot the window showed
  "Could not connect to localhost: Connection refused". A development build can now
  only disable an existing entry; enable autostart from the installed app.

### Changed

- README: download packages before installing them, with step-by-step update
  instructions per distribution.

## [0.1.0] - 2026-10-23

First public release.

### Added

- Native HID++ backend in Rust for the MX Master 3S over a Logi Bolt receiver: no
  Solaar or Python in the package.
- DPI, SmartShift, ratchet/free spin, wheel inversion and high resolution, thumb wheel
  inversion, battery level and charging state. Every change is confirmed by reading
  the mouse back.
- Back, Forward and gesture button remapping: native functions, held Ctrl/Alt,
  Copy/Paste/Cut/Undo, media keys, Hyprland workspace and window actions, and custom
  shortcuts.
- Directional gestures with the thumb button, with a distance that stays stable when
  DPI changes.
- Local profiles with export/import, reapply on reconnect and automatic switching per
  application on Hyprland.
- English and Spanish interface, chosen in **Preferences**; without a saved choice
  the app follows the system language, and the tray menu uses the same language.
- Interactive 3D mouse, light and dark themes, scaling for 4K screens, optional tray
  (click its icon to bring the window back) and autostart.
- Packages for Arch Linux, Debian/Ubuntu, Fedora and AppImage, built in CI, with a
  udev rule limited to the receiver's HID++ interface.

[Unreleased]: https://github.com/wh01s17/linux-scrollmaster/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/wh01s17/linux-scrollmaster/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/wh01s17/linux-scrollmaster/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/wh01s17/linux-scrollmaster/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/wh01s17/linux-scrollmaster/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/wh01s17/linux-scrollmaster/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/wh01s17/linux-scrollmaster/releases/tag/v0.1.0
