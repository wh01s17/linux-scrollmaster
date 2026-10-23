# Changelog

All notable changes to Linux ScrollMaster are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/wh01s17/linux-scrollmaster/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/wh01s17/linux-scrollmaster/releases/tag/v0.1.0
