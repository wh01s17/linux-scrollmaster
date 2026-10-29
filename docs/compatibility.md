# Compatibilidad Linux

## Matriz del MVP

| Área | Estado | Evidencia / límite |
|---|---|---|
| Arch, bibliotecas GTK/WebKit | Disponible | pkg-config GTK 3.24.52, WebKitGTK 2.52.6 |
| MX Master 3S por Bolt C548 | Verificado en hardware | Modelo B034, HID++ 4.5, batería 40 % durante sesión |
| Bluetooth | Implementación sin prueba física | Dispositivos directos de Solaar + bus 0005 en sysfs; no se alteró el emparejamiento del usuario |
| DPI | Escritura y lectura verificadas | 200–8000, paso 50 reportado por este mouse |
| SmartShift | Escritura y lectura verificadas | Rango Solaar 1–50; 50 corresponde a siempre dentado |
| MagSpeed | Escritura y lectura verificadas | Giro libre/dentado, inversión, resolución |
| Rueda horizontal | Inversión verificada | Velocidad/acciones alternativas pendientes |
| Botones secundarios | Asignación nativa implementada; catálogo leído en hardware | Mapeos nativos arbitrarios pendientes de aceptación; diversion temporal para acciones propias |
| Alt/Ctrl en laterales | Verificados en Hyprland/Wayland | Mantener/soltar probados; guardián restaura botones al cerrar o fallar worker |
| Copiar/Pegar/Cortar/Deshacer | Las cuatro acciones verificadas con usuario en la GUI | Combinaciones completas; dependen del atajo de la aplicación activa |
| Gestos direccionales | Copiar/Pegar izquierda/derecha verificados con usuario | raw XY y recuperación tras cierre/SIGKILL reales |
| Gestos multimedia | Seis acciones implementadas con pruebas | Volumen/reproducción confirmados; otras acciones pendientes; dependen del compositor/reproductor |
| Atajos personalizados | Implementados en laterales y gestos; aceptación física pendiente | Ctrl/Alt/Shift/Super y teclas del catálogo, keymap US; sin macros ni comandos |
| Acciones de espacios/ventanas | Implementadas y probadas con dobles; aceptación física pendiente | Cuatro opciones; Hyprland 0.55–0.57 Lua y teclado virtual Wayland |
| Perfiles por aplicación | Implementados para Hyprland | Foco y catálogo reales; cambio efectivo de mouse pendiente |
| Instancia única / bandeja | Implementadas y probadas automáticamente | Bandeja optativa y desactivada por defecto; aceptación gráfica pendiente |
| Inicio automático XDG | Implementado, optativo y probado en directorios temporales | Escritorio con autostart XDG; Hyprland por sí solo no basta; aceptación de inicio de sesión pendiente |
| Reconexión por apagado/encendido | Verificada con hardware y GUI | Perfil armado offline, escritura confirmada al reconectar, lectura independiente |
| Suspensión | Reanudación verificada con Linux ScrollMaster abierta | Journal y seis ajustes reales confirmados al volver |
| Wayland / Hyprland | Ventana y lectura real verificadas | NVIDIA requiere workaround WebKit integrado; Alt/Ctrl y edición reales |
| X11 | Diseño compatible, sin prueba de sesión | No se usa XTest ni xdotool |
| Otros modelos Logitech | Fuera del MVP | Filtro explícito por codename MX Master 3S |

La batería puede no estar disponible; se muestra «—», nunca un número simulado.
La misma identidad de dispositivo se usa para perfiles; si distintas conexiones
reportaran identidades distintas, habrá perfiles separados hasta verificarlo.

## Subsistemas y backends

- **HID++ 2.0 y posteriores:** features descubiertas dinámicamente; el índice de una
  feature no es su ID. La biblioteca Solaar gestiona transporte, receptor y consultas.
  No se asume un índice fijo ni que todo modelo soporte todos los ajustes.
- **Solaar:** herramienta mantenida con backend Python GPL-2.0-or-later; CLI config
  disponible, pero salida humana y persistencia asociada son inadecuadas como contrato.
  Se usa su biblioteca en proceso separado sin cargar perfiles del usuario. El adaptador v1
  acepta solo la versión probada 1.1.20. No hay
  una API pública estable que este MVP prometa soportar indefinidamente.
- **logiops/logid:** driver de usuario HID++ >2.0, configuración habitual en
  `/etc/logid.cfg`, acciones/gestos y D-Bus. Buena alternativa de backend futuro,
  pero no controlador simultáneo; depende de libevdev/libudev/libconfig/glib.
- **hidraw:** acceso a reports HID USB y Bluetooth, apto para configurar hardware.
- **evdev:** eventos de entrada Linux; captura global requiere permisos sensibles y
  puede duplicar eventos si no se coordina diversion/grab. No utilizado por Linux ScrollMaster hoy.
- **uinput:** crea dispositivos virtuales y permite enviar eventos a Wayland o X11;
  acceso sensible a `/dev/uinput`, no concedido por este MVP. Para un motor futuro,
  evaluar libevdev y un helper reducido, sin habilitar lectura de todo el teclado.
- **libinput:** traduce entrada para el compositor, aceleración y scroll. No sustituye
  HID++ ni ofrece a cualquier aplicación una API global de configuración del mouse.

## Hyprland y Omarchy

Una ventana Wayland no puede asumir observación global de teclas, foco ni inyección
de eventos mediante APIs de X11. La advertencia de Solaar sobre modificadores y
aplicación activa se refiere a su motor de reglas; no impide estas lecturas HID++.

Los perfiles por aplicación usan IPC oficial de Hyprland, con consultas JSON de
foco y lista de ventanas. La ruta se resuelve en cada petición; se contrastó con
la sesión real. Las reglas se evalúan en cada sondeo (5 s más consulta). No se
consumen todavía eventos socket2. Las acciones de espacios/ventanas tienen un
catálogo fijo separado, comprobación de capacidades y aceptación física pendiente.
Los atajos y multimedia usan teclado virtual Wayland autorizado, sin uinput.

No se editó `hyprland.conf`, configuración Omarchy, autostart ni reglas de entrada.
No se concedieron permisos uinput/evdev. Para hidraw, se recomienda instalar o
revisar las reglas distribuidas por Solaar y reconectar; no crear reglas 0666 globales.
Un receptor da acceso a más de un dispositivo emparejado: las ACL deben considerarlo.

## Fuentes primarias

- [Kernel: hidraw](https://www.kernel.org/doc/html/latest/hid/hidraw.html)
- [Kernel: uinput](https://docs.kernel.org/input/uinput.html)
- [libinput: propósito y límites](https://wayland.freedesktop.org/libinput/doc/latest/what-is-libinput.html)
- [Solaar](https://github.com/pwr-Solaar/Solaar), versión local 1.1.20 inspeccionada
- [logiops](https://github.com/PixlOne/logiops) y [configuración](https://github.com/PixlOne/logiops/wiki/Configuration)
- [Hyprland: IPC](https://wiki.hypr.land/IPC/)

Flow se mantiene como estudio: conmutación entre equipos y canales de transferencia
no equivalen a cambiar de host Easy-Switch. No se promete compatibilidad con el
protocolo propietario de Flow ni interoperabilidad con Options+.

## Renderizado NVIDIA / Wayland

En este equipo, WebKitGTK produjo Error 71 al abrir la ventana. El ajuste de proceso
`WEBKIT_DISABLE_DMABUF_RENDERER=1` resolvió la apertura; Linux ScrollMaster lo aplica solo si hay
Wayland y módulo NVIDIA cargado, salvo valor explícito del usuario. Puede reducir
el rendimiento gráfico; no cambia parámetros del kernel ni archivos del escritorio.
Fuente: [guía oficial de Tauri](https://v2.tauri.app/develop/debug/linux-graphics/).

Alt/Ctrl usa `zwp_virtual_keyboard_v1`; no presupone soporte en otros compositores
ni X11. No necesita uinput o permisos globales de teclado. Ver [held-modifiers.md](held-modifiers.md).
