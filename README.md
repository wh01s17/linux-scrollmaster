<div align="center">
  <img src="src-tauri/icons/icon.svg" width="96" height="96" alt="">
  <h1>Linux ScrollMaster</h1>
  <p>Independent Logitech MX Master 3S configurator for Linux</p>
  <a href="https://github.com/wh01s17/linux-scrollmaster/actions/workflows/ci.yml"><img src="https://github.com/wh01s17/linux-scrollmaster/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/wh01s17/linux-scrollmaster/releases/latest"><img src="https://img.shields.io/github/v/release/wh01s17/linux-scrollmaster" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue" alt="License"></a>
  <p><b>English</b> · <a href="README.es.md">Español</a></p>
</div>

An independent configurator for the **Logitech MX Master 3S** on Linux. Tune DPI,
wheels and buttons, assign shortcuts and gestures, and switch profiles based on the
focused application. It talks HID++ to the mouse directly from Rust: no account, no
cloud, no Solaar or Python in the native package, and the interface never runs as root.

Licensed **GPL-3.0-or-later**. A community project, not affiliated with Logitech
(see [Trademarks](#trademarks)). Changes per version are in the [changelog](CHANGELOG.md).

## Contents

- [Status](#status)
- [Features](#features)
- [Requirements](#requirements)
- [Installation](#installation)
  - [Arch Linux, Manjaro, EndeavourOS, Omarchy](#arch-linux-manjaro-endeavouros-omarchy)
  - [Debian, Ubuntu 22.04 or later, Linux Mint, Pop!_OS](#debian-ubuntu-2204-or-later-linux-mint-pop_os)
  - [Fedora and derivatives](#fedora-and-derivatives)
  - [Any distribution (AppImage)](#any-distribution-appimage)
  - [After installing](#after-installing)
  - [Verify the download (optional)](#verify-the-download-optional)
  - [Update](#update)
  - [Uninstall](#uninstall)
  - [Build from source](#build-from-source)
- [Usage](#usage)
- [Development](#development)
- [Contributing](#contributing)
- [Licenses](#licenses)
- [Trademarks](#trademarks)

## Status

Used daily with an **MX Master 3S** over a **Logi Bolt receiver** on Arch Linux with
Hyprland (Wayland). Every setting is confirmed by reading the mouse back, never
assumed to be applied.

| Area | Status |
|---|---|
| DPI, SmartShift, ratchet/free spin, wheel inversion and high resolution, thumb wheel inversion | Tested with the mouse |
| Battery level and charging state | Tested with the mouse |
| Back/Forward and gesture button: native functions, held Ctrl/Alt, Copy/Paste/Cut/Undo | Tested with the mouse |
| Directional gestures (action on start, speed-weighted travel, continuous volume) | Tested with the mouse |
| Local profiles, export/import and reapply on reconnect | Tested with the mouse |
| Per-application profiles on Hyprland | Tested (Firefox, kitty, VSCode) |
| Recovery after turning the mouse off, suspend, or an abrupt app exit | Tested: no key stays stuck |
| Custom shortcuts, maximize/fullscreen on Hyprland | Implemented; physical acceptance pending |
| Tray and autostart at login | Implemented; acceptance pending |
| Scroll distance per step (Hyprland only, via compositor) | Implemented; acceptance pending ([docs](docs/scroll-speed.md)) |
| Bluetooth | Detection implemented; **untested** |
| Other models or several receivers | Out of scope for now: no hardware to test them |

Details of what was verified and what is pending are in the
[acceptance protocol](docs/native-acceptance.md) and the [roadmap](docs/roadmap.md)
(both in Spanish).

## Features

- **Pointer:** DPI with a slider or an exact value, within the range reported by the mouse.
- **Scroll wheel:** ratchet mode with SmartShift or free spin, inversion and high resolution.
- **Thumb wheel:** direction inversion.
- **Buttons:** Back, Forward and the gesture button with their native function, another
  mouse function, held Ctrl or Alt, editing actions, media keys or a custom shortcut
  (Ctrl/Alt/Shift/Super + key).
- **Gestures:** hold the thumb button and move in five directions (up, down, left,
  right and no movement), with an adjustable distance that stays stable when DPI changes.
- **Desktop (Hyprland 0.55–0.57):** previous/next workspace, maximize and fullscreen.
- **Profiles:** automatic saving to Current profile, named copies, restore, rename, export and import; automatic recovery on reconnect;
  automatic switching by focused application on Hyprland.
- **Interface:** English or Spanish (chosen in Preferences), a 3D mouse that turns to
  the selected control, light and dark themes, scaling for 4K screens and an optional tray.

## Requirements

- Linux with Wayland. Keyboard actions use a Wayland virtual keyboard; per-application
  switching and desktop actions need **Hyprland**.
- An MX Master 3S with a **Logi Bolt** receiver.
- Access to the receiver's hidraw device for your user. The packages install a udev
  rule limited to the HID++ interface of the Bolt receiver (`046d:c548`, interface 02);
  it does not grant global access to input devices.
- No other mouse driver running: quit **Solaar** and stop **logid** before opening the
  app. Linux ScrollMaster does not stop services for you.

## Installation

Each release publishes packages on [Releases](https://github.com/wh01s17/linux-scrollmaster/releases),
built with the native backend. The commands below use version **0.3.2**; for a later
release, change the number in the URL and the file name.

### Arch Linux, Manjaro, EndeavourOS, Omarchy

```sh
curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/v0.3.2/linux-scrollmaster-native-0.3.2-1-x86_64.pkg.tar.zst
sudo pacman -U ./linux-scrollmaster-native-0.3.2-1-x86_64.pkg.tar.zst
```

This is the package tested with the mouse. Download it first: installing straight
from the URL makes pacman ask for a `.sig` signature, and the packages are not signed.

### Debian, Ubuntu 22.04 or later, Linux Mint, Pop!_OS

```sh
curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/v0.3.2/linux-scrollmaster_0.3.2_amd64.deb
sudo apt install ./linux-scrollmaster_0.3.2_amd64.deb
```

### Fedora and derivatives

```sh
curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/v0.3.2/linux-scrollmaster-0.3.2-1.x86_64.rpm
sudo dnf install ./linux-scrollmaster-0.3.2-1.x86_64.rpm
```

### Any distribution (AppImage)

```sh
curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/v0.3.2/linux-scrollmaster_0.3.2_amd64.AppImage
chmod +x linux-scrollmaster_0.3.2_amd64.AppImage
curl -fsSLO https://raw.githubusercontent.com/wh01s17/linux-scrollmaster/main/packaging/udev/70-linux-scrollmaster.rules
sudo install -Dm644 70-linux-scrollmaster.rules /usr/lib/udev/rules.d/70-linux-scrollmaster.rules
sudo udevadm control --reload
./linux-scrollmaster_0.3.2_amd64.AppImage
```

The AppImage does not install the udev rule; that is what the three middle commands
do, once.

### After installing

1. **Unplug and replug the Bolt receiver** so the udev rule gives your user access.
   Rebooting works too.
2. **Quit Solaar and stop logid** if you use them. Two programs talking to the mouse at
   the same time overwrite each other's settings.
3. Open **Linux ScrollMaster** from your application menu. The language follows your
   system; change it in **Preferences**.

If the app says it has no permission to open the receiver, repeat step 1. The rule
grants access to the user of the active graphical session, not to other accounts or
remote sessions.

### Verify the download (optional)

Each release includes `SHA256SUMS`. In the folder where you downloaded the package:

```sh
curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/v0.3.2/SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS
```

### Update

The app does not update itself. To move to a new release:

1. Quit Linux ScrollMaster, including from the tray (**Quit Linux ScrollMaster**), so
   the old version does not keep running.
2. Download the new package from [Releases](https://github.com/wh01s17/linux-scrollmaster/releases/latest) and install it with the same
   command as for a first install. Replace `X.Y.Z` with the new version:

   | System | Commands |
   |---|---|
   | Arch | `curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/vX.Y.Z/linux-scrollmaster-native-X.Y.Z-1-x86_64.pkg.tar.zst`<br>`sudo pacman -U ./linux-scrollmaster-native-X.Y.Z-1-x86_64.pkg.tar.zst` |
   | Debian/Ubuntu | `curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/vX.Y.Z/linux-scrollmaster_X.Y.Z_amd64.deb`<br>`sudo apt install ./linux-scrollmaster_X.Y.Z_amd64.deb` |
   | Fedora | `curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/vX.Y.Z/linux-scrollmaster-X.Y.Z-1.x86_64.rpm`<br>`sudo dnf install ./linux-scrollmaster-X.Y.Z-1.x86_64.rpm` |
   | AppImage | download the new `.AppImage`, make it executable and delete the old one |

3. Open the app again.

Profiles and preferences in `~/.config/linux-scrollmaster/` are kept. Check the
[changelog](CHANGELOG.md) before updating: it says if a release needs anything else,
such as replugging the receiver after a udev rule change.

### Uninstall

| System | Command |
|---|---|
| Arch | `sudo pacman -R linux-scrollmaster-native` |
| Debian/Ubuntu | `sudo apt remove linux-scroll-master` |
| Fedora | `sudo dnf remove linux-scroll-master` |
| AppImage | delete the file and `/usr/lib/udev/rules.d/70-linux-scrollmaster.rules` |

Profiles stay in `~/.config/linux-scrollmaster/`; delete that folder if you do not want to keep them.

Only the Arch package has been tested with the mouse. The `.deb`, `.rpm` and AppImage
packages are built in CI and install cleanly, but have no physical acceptance yet:
reports are welcome.

### Build from source

**Arch:** build the package locally from a clean commit.

```sh
git clone https://github.com/wh01s17/linux-scrollmaster
cd linux-scrollmaster
python packaging/arch/prepare.py --variant native /tmp/scrollmaster-native-kit
cd /tmp/scrollmaster-native-kit
makepkg -f
sudo pacman -U linux-scrollmaster-native-*.pkg.tar.zst
```

If Node.js or pnpm come from a version manager (mise, nvm) rather than pacman, use
`makepkg -f -d`.

**Other distributions:** you need Node.js 22.12+ or 24, pnpm, Rust 1.89+, a C compiler,
pkg-config and the development libraries for GTK3, WebKitGTK 4.1, libsoup3, OpenSSL,
Wayland, libxkbcommon and libayatana-appindicator.

```sh
pnpm install --frozen-lockfile
pnpm tauri build --no-bundle --features native-default
sudo install -Dm644 packaging/udev/70-linux-scrollmaster.rules /usr/lib/udev/rules.d/70-linux-scrollmaster.rules
sudo udevadm control --reload
./src-tauri/target/release/linux-scrollmaster
```

## Usage

In **Preferences**, enable **Start in the notification area and keep running when
closed** to start with the window hidden. Click the tray icon or launch the app
again to show it. Enable **Open Linux ScrollMaster at login** separately to start
with your session. If the tray cannot be created after about 10 seconds of retries,
the window appears. These options are off by default.

1. Quit Solaar or logid if you use them.
2. Open Linux ScrollMaster with the mouse on.
3. Pick a control on the 3D mouse or in the tabs. Changes apply when you release the
   control and are confirmed by reading the mouse back.
4. In **Buttons and gestures**, assign functions or shortcuts. "Test assignments" lets
   you check Alt/Ctrl and Copy/Paste without leaving the app.
5. Changes save automatically to **Current profile**, including buttons, shortcuts
   and gestures. Use **Save as…** to keep a named copy or assign one to an application.
   Your saved configuration loads on startup and reconnect; there is no opt-in checkbox.

Keyboard actions and gestures work while the app is open, or from the tray if you
enable that preference. When you quit, the app releases the buttons it captured and
those actions stop. Hardware settings (DPI, wheels, native remapping) stay on the mouse
until its firmware resets them: the MX Master 3S goes back to 1000 DPI when turned off,
which is why Linux ScrollMaster restores your saved configuration automatically.

Saving reuses the confirmed device reading; keeping a named copy does not query
the mouse again. See the [automatic profile contract](docs/automatic-profiles.md)
for failure handling and the pending physical acceptance of this new flow.

Profiles are stored in `$XDG_CONFIG_HOME/linux-scrollmaster/profiles.json` (usually
`~/.config/linux-scrollmaster/profiles.json`).

## Development

```sh
pnpm install --frozen-lockfile
SCROLLMASTER_BACKEND=native pnpm tauri dev
```

In development the default backend is still the old Solaar 1.1.20 adapter
(`bridge/`), which will be removed; `SCROLLMASTER_BACKEND=native` uses the Rust
backend, the same one shipped in the packages. `pnpm dev` opens only the interface in
a browser, which has no mouse access and says so on screen.

Tests, with no mouse, root, network or hidraw permissions:

```sh
pnpm test
pnpm build
cargo check --offline --manifest-path src-tauri/Cargo.toml
cargo test --offline --manifest-path src-tauri/Cargo.toml
```

Hardware tests (`tests/hardware_*.py` and the `native_*` Cargo examples) are optional:
they change the mouse temporarily and restore its values when done.

Interface texts live in `src/locales/es.ts` (source) and `src/locales/en.ts`; a test
checks that both have the same keys and placeholders. Labels and messages from the
Rust backend are Spanish and are mapped to English in `src/locales/en.ts`.

Technical docs, in Spanish: [architecture](docs/architecture.md),
[native backend](docs/native-backend.md), [interface integration](docs/native-gui.md),
[compatibility](docs/compatibility.md), [testing](docs/testing.md) and the
[development plan](plan.md). The project's working rules are in `AGENTS.md`.

The 3D mouse is rendered with `art/mouse-render/` from the model's STEP files (not
included; download them from the original page and place them in `3d/`).

## Contributing

Reports and pull requests are welcome, in English or Spanish. When reporting a problem,
include your distribution, compositor, mouse model, connection type (Bolt or Bluetooth)
and the steps to reproduce it. If you have another Logitech HID++ mouse, diagnostic
readings help widen compatibility, but do not promise support.

The project does not use or include Logitech software, binaries, images or fonts, and
does not accept contributions that do. The protocol is implemented from public
documentation and free projects such as Solaar and logiops.

## Licenses

- Code: **GPL-3.0-or-later** ([LICENSE](LICENSE)).
- 3D mouse images (`src/mouse3d/frames/`): derived from the "Replacement shell for
  Logitech MX Master 3 and 3S" by **Ziemniaczenka**
  ([MakerWorld](https://makerworld.com/en/models/1150456)), distributed under
  **CC BY-NC-SA 4.0**, not under the GPL: see [ASSETS-LICENSE](src/mouse3d/ASSETS-LICENSE.md).
  Commercial use is not allowed.

## Trademarks

Linux ScrollMaster is an independent project and is not affiliated with, endorsed or
sponsored by Logitech. Logitech, Logi, MX Master, Logi Bolt, Logi Options+ and
SmartShift are trademarks of Logitech International S.A. They are mentioned only to
state which hardware the program works with.
