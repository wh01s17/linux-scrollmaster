<div align="center">
  <img src="src-tauri/icons/icon.svg" width="96" height="96" alt="">
  <h1>Linux ScrollMaster</h1>
  <p>Configurador independiente del Logitech MX Master 3S para Linux</p>
  <a href="https://github.com/wh01s17/linux-scrollmaster/actions/workflows/ci.yml"><img src="https://github.com/wh01s17/linux-scrollmaster/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/wh01s17/linux-scrollmaster/releases/latest"><img src="https://img.shields.io/github/v/release/wh01s17/linux-scrollmaster" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue" alt="License"></a>
  <p><a href="README.md">English</a> · <b>Español</b></p>
</div>

Configurador independiente del **Logitech MX Master 3S** para Linux. Ajusta DPI,
ruedas y botones, asigna atajos y gestos, y cambia de perfil según la aplicación
activa. Habla HID++ directamente con el mouse desde Rust: sin cuenta, sin nube,
sin Solaar ni Python en el paquete nativo, y sin ejecutar la interfaz como root.

Licencia **GPL-3.0-or-later**. Proyecto comunitario, sin relación con Logitech
(ver [Marcas](#marcas)). Los cambios de cada versión están en el [registro de cambios](CHANGELOG.md), en inglés.

## Índice

- [Estado](#estado)
- [Funciones](#funciones)
- [Requisitos](#requisitos)
- [Instalación](#instalación)
  - [Arch Linux, Manjaro, EndeavourOS, Omarchy](#arch-linux-manjaro-endeavouros-omarchy)
  - [Debian, Ubuntu 22.04 o superior, Linux Mint, Pop!_OS](#debian-ubuntu-2204-o-superior-linux-mint-pop_os)
  - [Fedora y derivadas](#fedora-y-derivadas)
  - [Cualquier distribución (AppImage)](#cualquier-distribución-appimage)
  - [Después de instalar](#después-de-instalar)
  - [Verificar la descarga (opcional)](#verificar-la-descarga-opcional)
  - [Actualizar](#actualizar)
  - [Desinstalar](#desinstalar)
  - [Compilar desde el código](#compilar-desde-el-código)
- [Uso](#uso)
- [Desarrollo](#desarrollo)
- [Contribuir](#contribuir)
- [Licencias](#licencias)
- [Marcas](#marcas)

## Estado

Se usa a diario con un **MX Master 3S** conectado por **receptor Logi Bolt** en
Arch Linux con Hyprland (Wayland). Cada ajuste se confirma leyendo de nuevo el
mouse, no se da por aplicado.

| Área | Estado |
|---|---|
| DPI, SmartShift, modo dentado/giro libre, inversión y alta resolución de la rueda, inversión de la rueda lateral | Probado con el mouse |
| Batería y estado de carga | Probado con el mouse |
| Atrás/Adelante y botón de gestos: funciones nativas, Ctrl/Alt mantenidos, Copiar/Pegar/Cortar/Deshacer | Probado con el mouse |
| Gestos direccionales (acción al iniciar, respuesta por velocidad, volumen continuo) | Probado con el mouse |
| Perfiles locales, exportar/importar y reaplicar al reconectar | Probado con el mouse |
| Perfiles por aplicación en Hyprland | Probado (Firefox, kitty, VSCode) |
| Recuperación tras apagar el mouse, suspender o cerrar la app de forma abrupta | Probado: ninguna tecla queda pegada |
| Atajos personalizados, maximizar/pantalla completa en Hyprland | Implementado; aceptación física pendiente |
| Bandeja e inicio automático al iniciar sesión | Implementado; aceptación gráfica pendiente |
| Distancia de desplazamiento por paso (solo Hyprland, vía compositor) | Implementado; aceptación gráfica pendiente ([docs](docs/scroll-speed.md)) |
| Bluetooth | Detección implementada; **sin probar** |
| Otros modelos o varios receptores | Fuera de alcance por ahora: no hay hardware para probarlos |

El detalle de lo comprobado y lo pendiente está en el
[protocolo de aceptación](docs/native-acceptance.md) y la [hoja de ruta](docs/roadmap.md).

## Funciones

- **Movimiento:** DPI con deslizador o valor exacto, dentro del rango que informa el mouse.
- **Rueda vertical:** modo dentado con SmartShift o giro libre, inversión y alta resolución.
- **Rueda lateral:** inversión de dirección.
- **Botones:** Atrás, Adelante y botón de gestos con su función nativa, otra función
  del mouse, Ctrl o Alt mantenidos, acciones de edición, multimedia o un atajo
  personalizado (Ctrl/Alt/Shift/Super + tecla).
- **Gestos:** mantener el botón del pulgar y mover en cinco direcciones (arriba,
  abajo, izquierda, derecha y sin movimiento), con distancia ajustable y estable
  al cambiar los DPI.
- **Escritorio (Hyprland 0.55–0.57):** espacio anterior/siguiente, maximizar y pantalla completa.
- **Perfiles:** guardar, restaurar, renombrar, exportar e importar; reaplicar al
  reconectar; cambio automático por aplicación enfocada en Hyprland.
- **Interfaz:** en español o inglés (se elige en Preferencias), mouse 3D que gira
  hacia el control elegido, tema claro y oscuro, escalado para pantallas 4K y
  bandeja opcional.

## Requisitos

- Linux con Wayland. Las acciones de teclado usan un teclado virtual Wayland; el
  cambio por aplicación y las acciones de escritorio requieren **Hyprland**.
- MX Master 3S con receptor **Logi Bolt**.
- Acceso al dispositivo hidraw del receptor para tu usuario. El paquete instala una
  regla udev acotada a la interfaz HID++ del receptor Bolt (`046d:c548`, interfaz 02);
  no concede acceso global a dispositivos de entrada.
- Ningún otro controlador del mouse en ejecución: cierra **Solaar** y detén **logid**
  antes de abrir la app. Linux ScrollMaster no detiene servicios por ti.

## Instalación

Los paquetes de cada versión están en [Releases](https://github.com/wh01s17/linux-scrollmaster/releases),
compilados con el backend nativo. Los comandos usan la versión **0.3.1**; para una
versión posterior cambia el número en la URL y en el nombre del archivo.

### Arch Linux, Manjaro, EndeavourOS, Omarchy

```sh
curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/v0.3.1/linux-scrollmaster-native-0.3.1-1-x86_64.pkg.tar.zst
sudo pacman -U ./linux-scrollmaster-native-0.3.1-1-x86_64.pkg.tar.zst
```

Este es el paquete probado con el mouse. Descárgalo primero: si instalas directo desde
la URL, pacman pide una firma `.sig`, y los paquetes no están firmados.

### Debian, Ubuntu 22.04 o superior, Linux Mint, Pop!_OS

```sh
curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/v0.3.1/linux-scrollmaster_0.3.1_amd64.deb
sudo apt install ./linux-scrollmaster_0.3.1_amd64.deb
```

### Fedora y derivadas

```sh
curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/v0.3.1/linux-scrollmaster-0.3.1-1.x86_64.rpm
sudo dnf install ./linux-scrollmaster-0.3.1-1.x86_64.rpm
```

### Cualquier distribución (AppImage)

```sh
curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/v0.3.1/linux-scrollmaster_0.3.1_amd64.AppImage
chmod +x linux-scrollmaster_0.3.1_amd64.AppImage
curl -fsSLO https://raw.githubusercontent.com/wh01s17/linux-scrollmaster/main/packaging/udev/70-linux-scrollmaster.rules
sudo install -Dm644 70-linux-scrollmaster.rules /usr/lib/udev/rules.d/70-linux-scrollmaster.rules
sudo udevadm control --reload
./linux-scrollmaster_0.3.1_amd64.AppImage
```

La AppImage no instala la regla udev; por eso van los tres comandos del medio, una
sola vez.

### Después de instalar

1. **Desconecta y vuelve a conectar el receptor Bolt**, para que la regla udev le dé
   acceso a tu usuario. Reiniciar también sirve.
2. **Cierra Solaar y detén logid** si los usas. Dos programas hablando con el mouse a
   la vez se pisan los ajustes.
3. Abre **Linux ScrollMaster** desde el menú de aplicaciones.

Si la app dice que no tiene permiso para abrir el receptor, repite el paso 1. La
regla da acceso a quien tiene la sesión gráfica activa, no a otras cuentas ni a
sesiones remotas.

### Verificar la descarga (opcional)

Cada release incluye `SHA256SUMS`. En la carpeta donde descargaste el paquete:

```sh
curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/v0.3.1/SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS
```

### Actualizar

La app no se actualiza sola. Para pasar a una versión nueva:

1. Cierra Linux ScrollMaster, también desde la bandeja (**Salir de Linux ScrollMaster**),
   para que no siga corriendo la versión anterior.
2. Descarga el paquete nuevo desde [Releases](https://github.com/wh01s17/linux-scrollmaster/releases/latest) e instálalo con el mismo
   comando de la primera instalación. Cambia `X.Y.Z` por la versión nueva:

   | Sistema | Comandos |
   |---|---|
   | Arch | `curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/vX.Y.Z/linux-scrollmaster-native-X.Y.Z-1-x86_64.pkg.tar.zst`<br>`sudo pacman -U ./linux-scrollmaster-native-X.Y.Z-1-x86_64.pkg.tar.zst` |
   | Debian/Ubuntu | `curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/vX.Y.Z/linux-scrollmaster_X.Y.Z_amd64.deb`<br>`sudo apt install ./linux-scrollmaster_X.Y.Z_amd64.deb` |
   | Fedora | `curl -fLO https://github.com/wh01s17/linux-scrollmaster/releases/download/vX.Y.Z/linux-scrollmaster-X.Y.Z-1.x86_64.rpm`<br>`sudo dnf install ./linux-scrollmaster-X.Y.Z-1.x86_64.rpm` |
   | AppImage | descarga el `.AppImage` nuevo, dale permiso de ejecución y borra el anterior |

3. Abre la app de nuevo.

Los perfiles y preferencias de `~/.config/linux-scrollmaster/` se conservan. Revisa el
[registro de cambios](CHANGELOG.md) antes de actualizar: indica si una versión necesita
algo más, como reconectar el receptor tras un cambio en la regla udev.

### Desinstalar

| Sistema | Comando |
|---|---|
| Arch | `sudo pacman -R linux-scrollmaster-native` |
| Debian/Ubuntu | `sudo apt remove linux-scroll-master` |
| Fedora | `sudo dnf remove linux-scroll-master` |
| AppImage | borra el archivo y `/usr/lib/udev/rules.d/70-linux-scrollmaster.rules` |

Los perfiles quedan en `~/.config/linux-scrollmaster/`; bórrala si no los quieres conservar.

Solo el paquete de Arch se ha probado con el mouse. Los paquetes `.deb`, `.rpm` y
AppImage se generan en CI y todavía no tienen aceptación física: los reportes son
bienvenidos.

### Compilar desde el código

**Arch:** genera el paquete localmente desde un commit limpio.

```sh
git clone https://github.com/wh01s17/linux-scrollmaster
cd linux-scrollmaster
python packaging/arch/prepare.py --variant native /tmp/scrollmaster-native-kit
cd /tmp/scrollmaster-native-kit
makepkg -f
sudo pacman -U linux-scrollmaster-native-*.pkg.tar.zst
```

Si Node.js o pnpm vienen de un gestor de versiones (mise, nvm) y no de pacman, usa
`makepkg -f -d`.

**Otras distribuciones:** necesitas Node.js 22.12+ o 24, pnpm, Rust 1.89+, un
compilador C, pkg-config y las bibliotecas de desarrollo de GTK3, WebKitGTK 4.1,
libsoup3, OpenSSL, Wayland, libxkbcommon y libayatana-appindicator.

```sh
pnpm install --frozen-lockfile
pnpm tauri build --no-bundle --features native-default
sudo install -Dm644 packaging/udev/70-linux-scrollmaster.rules /usr/lib/udev/rules.d/70-linux-scrollmaster.rules
sudo udevadm control --reload
./src-tauri/target/release/linux-scrollmaster
```

## Uso

En **Preferencias**, activa **Iniciar en el área de notificaciones y continuar al
cerrar** para arrancar con la ventana oculta. Un clic en el icono o abrir la app
de nuevo muestra la ventana. Activa además **Abrir Linux ScrollMaster al iniciar
sesión** si quieres que arranque con tu sesión. Si no se puede crear el icono tras
unos 10 s de reintentos, se muestra la ventana. Ambas opciones vienen desactivadas.

1. Cierra Solaar o logid si los usas.
2. Abre Linux ScrollMaster con el mouse encendido.
3. Elige un control en el mouse 3D o en las pestañas. Los cambios se aplican al
   soltar el control y se confirman leyendo el mouse.
4. En **Botones y gestos** asigna funciones o atajos. «Probar asignaciones» permite
   comprobar Alt/Ctrl y Copiar/Pegar sin salir de la app.
5. Cada cambio se guarda automáticamente en **Perfil actual**, incluidos botones,
   atajos y gestos. Usa **Guardar como…** para conservar una copia con nombre o
   asignarla a una aplicación. La configuración se carga al abrir y reconectar,
   sin casilla optativa.

Las acciones de teclado y los gestos funcionan mientras la app está abierta, o en
la bandeja si activas esa preferencia. Al salir, la app libera los botones que
capturaba y esas acciones dejan de funcionar. Los ajustes de hardware (DPI, ruedas,
reasignaciones nativas) quedan en el mouse hasta que su firmware los restablezca:
el MX Master 3S vuelve a 1000 DPI al apagarse, y por eso Linux ScrollMaster
recupera automáticamente la configuración guardada.

Guardar aprovecha la lectura que confirma el cambio; crear una copia con nombre
no vuelve a consultar el mouse. El [contrato de guardado automático](docs/automatic-profiles.md)
detalla el manejo de fallos y la aceptación física pendiente de este flujo nuevo.

Los perfiles se guardan en `$XDG_CONFIG_HOME/linux-scrollmaster/profiles.json` (normalmente
`~/.config/linux-scrollmaster/profiles.json`).

## Desarrollo

```sh
pnpm install --frozen-lockfile
SCROLLMASTER_BACKEND=native pnpm tauri dev
```

En modo desarrollo el backend predeterminado todavía es el adaptador antiguo con
Solaar 1.1.20 (`bridge/`), que se retirará; `SCROLLMASTER_BACKEND=native` usa el
backend Rust, el mismo del paquete. `pnpm dev` abre solo la interfaz en el
navegador, que no tiene acceso al mouse y lo indica en pantalla.

Pruebas, sin mouse, root, red ni permisos hidraw:

```sh
pnpm test
pnpm build
cargo check --offline --manifest-path src-tauri/Cargo.toml
cargo test --offline --manifest-path src-tauri/Cargo.toml
```

Las pruebas con hardware (`tests/hardware_*.py` y los ejemplos `native_*` de Cargo)
son optativas: cambian temporalmente el mouse y restauran sus valores al terminar.

Documentación técnica: [arquitectura](docs/architecture.md),
[backend nativo](docs/native-backend.md), [integración con la interfaz](docs/native-gui.md),
[compatibilidad](docs/compatibility.md), [pruebas](docs/testing.md) y el
[plan de desarrollo](plan.md). Las reglas de trabajo del proyecto están en `AGENTS.md`.

El mouse 3D se genera con `art/mouse-render/` a partir de los archivos STEP del
modelo (no incluidos; se descargan de su página original y van en `3d/`).

## Contribuir

Los reportes y pull requests son bienvenidos. Al reportar un problema indica
distribución, compositor, modelo del mouse, tipo de conexión (Bolt o Bluetooth) y
los pasos para reproducirlo. Si tienes otro mouse Logitech con HID++, las lecturas
de diagnóstico ayudan a ampliar la compatibilidad, pero no prometen soporte.

El proyecto no usa ni incluye software, binarios, imágenes ni tipografías de
Logitech, y no acepta contribuciones que los incluyan. El protocolo se implementa
a partir de documentación pública y de proyectos libres como Solaar y logiops.

## Licencias

- Código: **GPL-3.0-or-later** ([LICENSE](LICENSE)).
- Imágenes del mouse 3D (`src/mouse3d/frames/`): obra derivada de la carcasa
  «Replacement shell for Logitech MX Master 3 and 3S» de **Ziemniaczenka**
  ([MakerWorld](https://makerworld.com/en/models/1150456)), distribuida bajo
  **CC BY-NC-SA 4.0**, no bajo la GPL: ver [ASSETS-LICENSE](src/mouse3d/ASSETS-LICENSE.md).
  Su uso comercial no está permitido.

## Marcas

Linux ScrollMaster es un proyecto independiente y no está afiliado, respaldado ni
patrocinado por Logitech. Logitech, Logi, MX Master, Logi Bolt, Logi Options+ y
SmartShift son marcas de Logitech International S.A. Se mencionan solo para
indicar con qué hardware funciona el programa.
