# Distancia de desplazamiento (Hyprland)

Las pestañas «Rueda vertical» y «Rueda lateral» muestran «Distancia de desplazamiento»:
un deslizador de 0,1 a 5 (paso 0,05) y un campo exacto, como los DPI. Un solo factor
cubre ambas ruedas, porque Hyprland aplica `scroll_factor` a los dos ejes.

## Por qué en el compositor

El MX Master 3S no tiene control HID++ de distancia por paso: 0x2121 y 0x2150 solo
ofrecen modo, resolución e inversión. La «velocidad» de Logi Options+ es software que
desvía la rueda y reinyecta eventos; en Linux exigiría `uinput` y diversion, descartados
por permisos globales de entrada y por el riesgo de dejar la rueda sin consumidor.

Hyprland 0.55/0.56 admite `scroll_factor` por dispositivo (`hl.device`). Decisión del
usuario (2026-10-24): ofrecerlo solo en Hyprland y aplicarlo en tiempo de ejecución por
IPC. Se cambia la configuración en memoria de la sesión, nunca archivos de
`~/.config/hypr` ni de Omarchy. Fuera de Hyprland el control no aparece.

## Implementación

- `src-tauri/src/native/scroll_factor.rs`: comprueba versión 0.55/0.56 y `hl.device`,
  lee `j/devices` y elige solo `logitech-usb-receiver-mouse` (Bolt) o nombres con
  `mx-master-3s` (Bluetooth), con nombres restringidos a `[A-Za-z0-9-_.:]` antes de
  entrar en Lua. Envía `/eval hl.device({ name = "…", scroll_factor = N.NN })` por
  dispositivo y confirma con una lectura nueva de `scrollFactor`.
- Nunca se envía `false` ni 0: en Hyprland 0.56.2 `scroll_factor = false` dejó el factor
  en 0 (sin desplazamiento). Valores negativos se rechazan por Hyprland.
- `src-tauri/src/scroll_speed.rs`: guarda solo `linux-scrollmaster/scroll.json`
  (`{"version":1,"factor":N}`), con el mismo criterio que `desktop.json`: un archivo
  corrupto se conserva y bloquea escrituras. «Restablecer» borra ese archivo y aplica el
  `input:scroll_factor` global de la sesión. Cada 5 s, si hay preferencia guardada y
  Hyprland muestra otro valor (p. ej. tras `hyprctl reload`), se reaplica.
- `src/scrollSpeed.ts` y `ScrollSpeed.vue`: estado separado del mouse; muestra el valor
  confirmado, valida el rango antes de escribir y relee tras un error.

## Verificación

- Rust: 7 pruebas con socket sintético (rango, objetivos, payload, confirmación,
  versión no compatible, mouse ausente, lectura global) y 3 de preferencia/descripción.
- Vue: 6 pruebas del estado (vista web, fuera de Hyprland, escritura confirmada,
  restablecer, rango, error con relectura, respuesta inválida).
- Contra Hyprland 0.56.2 real, con factor 1.00 (equivalente al global): las cargas
  exactas de sondeo, `hl.device`, `j/getoption` y `j/devices` respondieron como se espera.

## Limitaciones

- Un receptor Bolt con varios mouse Logitech comparte el nombre
  `logitech-usb-receiver-mouse`: el factor se aplica a todos ellos.
- Sin Linux ScrollMaster abierto, un `hyprctl reload` vuelve al valor global hasta el
  próximo inicio. Pendiente aceptación gráfica en la app instalada.
