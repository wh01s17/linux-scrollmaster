# Alt y Ctrl mantenidos en botones laterales

Sesión: 17 de octubre de 2026, America/Santiago; evidencia UTC del 18.

## Resultado

**Implementado y verificado con el usuario en MX Master 3S/Bolt + Hyprland/Wayland.**
Atrás y Adelante ofrecen Alt y Ctrl como opciones operativas. El modificador se
mantiene mientras se presiona el botón y se libera al soltarlo. El usuario confirmó
ambas teclas y su liberación mediante eventos reales de teclado en la interfaz.
Las opciones ya no son placeholders ni valores ficticios de un destino HID++.

No se asignan automáticamente. La prueba GUI cambió temporalmente Atrás a Ctrl
y Adelante a Alt. Al terminar se cerró la instancia de prueba y se comprobó
independientemente la vuelta a Atrás/Adelante originales; Linux ScrollMaster se reabrió.
No se sobrescribieron perfiles ni se habilitó reaplicación.

## Arquitectura

1. `bridge/solaar_adapter.py` comprueba capacidad DIVERTABLE y reporting real.
   Bloquea clics principales, botón de gestos como origen de modificadores y
   controles ya interceptados que no pertenezcan a esta sesión.
2. Antes de desviar un botón, inicia un guardián y verifica que puede crear un
   teclado virtual autorizado por el compositor. El guardián registra el CID y
   confirma su recepción antes de que el worker active diversion temporal.
3. Las llamadas HID++ y los eventos usan el mismo descriptor. Un wrapper ofrece
   `notifications_hook` para capturar también eventos encontrados al esperar una
   respuesta. El bucle del servicio consume notificaciones cuando está inactivo;
   no hay dos lectores concurrentes del mismo transporte.
4. Los informes 1B04 de controles mantenidos contienen hasta cuatro CIDs. El motor
   `ModifierKeys` calcula el conjunto de modificadores; soporta Alt+Ctrl y dos
   botones asignados al mismo modificador sin liberarlo prematuramente.
5. `bridge/modifiers.py` comunica el estado al guardián. La biblioteca propia
   `native/keyboard.c` implementa el cliente `zwp_virtual_keyboard_v1` usando
   libwayland-client y libxkbcommon. Solo envía estado Alt/Ctrl, no texto arbitrario.
6. El guardián tiene un descriptor hidraw heredado, pero no lo lee mientras el
   worker funciona. Ante EOF libera el teclado y desactiva diversion de los CIDs
   registrados. Sobrevive a SIGKILL del worker y puede restaurar el transporte.
7. Mientras hay una sesión de modificadores, el worker conserva el flock del
   mouse entre solicitudes. Otros procesos cooperativos fallan con ocupado. El
   observador independiente usa bloqueo no bloqueante y no espera indefinidamente.

Volver a una función nativa libera el modificador, desactiva diversion y confirma
el reporting antes de continuar. Sin bindings restantes se cierra el guardián.
Desconexión/notificación de enlace y pausas prolongadas del bucle liberan estado;
los errores del consumidor cierran el backend y disparan recuperación.

No se activa diversion persistente ni raw XY. No se accede a evdev, uinput, teclas
globales del usuario, configuración de Hyprland, servicios o reglas de permisos.
La GUI y todos los procesos funcionan como usuario normal.

## Persistencia y UI

- Acciones tipadas `modifier:alt` y `modifier:ctrl`; no se convierten a números.
- Schema 3 añade esos destinos a los botones 83/86. Los documentos 1/2 siguen
  siendo válidos. Guardar un perfil con modificadores migra al 3 sin degradar
  otros perfiles existentes; leer no provoca migraciones ni escrituras.
- Los perfiles anteriores sin asignaciones no cambian botones. Guardado,
  restauración y reconciliación con modificadores están probados con dobles.
- La UI solo ofrece las opciones entregadas por el backend residente. La ruta
  one-shot rechaza modificadores al no tener consumidor permanente.
- «Probar Alt y Ctrl» captura exclusivamente la tecla recibida en su campo con
  foco. Muestra los flags reales de la última pulsación; no es una simulación de
  la asignación elegida ni un monitor global de teclado.

## Evidencia de pruebas

| Caso | Resultado |
|---|---|
| `python tests/wayland_keyboard_probe.py` | PASS real: teclado virtual autorizado, estado inicial cero |
| `python tests/hardware_modifiers_roundtrip.py` | PASS real: armó ambos botones y confirmó reporting |
| Cierre normal del worker | PASS real: funciones y flags originales restaurados |
| SIGKILL del worker | PASS real: guardián restauró ambos botones tras morir el padre |
| Perfiles después del script | Sin cambios, comprobado por igualdad del documento |
| GUI: Ctrl mantenido y liberado | Usuario confirmó funcionamiento |
| GUI: Alt mantenido y liberado | Usuario confirmó funcionamiento |
| Cierre de la app de prueba | Lectura independiente 02:17:22 UTC: 83→83, 86→86, 195→195, disponibles |
| Suite regular | 81 PASS: 64 Python, 14 estado Vue, 3 Rust |
| Build y checks | pnpm build, Tauri debug sin bundle, cargo check/test offline, fmt y sintaxis PKGBUILD aprobados |

La observación final también leyó DPI 1850, SmartShift 10, modo 2, inversión
vertical false, alta resolución false y lateral true. El perfil «Mi perfil»
seguía en schema 1 con alta resolución true y `active` vacío. No se impuso el
valor del perfil al dispositivo ni se atribuye ese cambio de resolución al
motor de modificadores, que no escribe ajustes escalares.

## Límites y pendientes

- Probado en Hyprland/Wayland. Otros compositores deben autorizar ese protocolo;
  si falta, se muestra error y no se desvía el botón. X11 no está implementado.
- Actualmente una sesión admite modificadores en un solo mouse a la vez.
- Solo funciona mientras Linux ScrollMaster está abierta; cerrar devuelve funciones nativas.
  Para reactivar al abrir/reconectar, guardar y armar un perfil expresamente.
- Guardar/restaurar/reconectar un perfil schema 3 está probado automáticamente;
  falta su aceptación física completa. La aceptación previa Bolt de los seis
  ajustes no se extiende automáticamente a este nuevo caso.
- No se probó desconexión física mientras un modificador permanece pulsado.
- La recuperación comprobada cubre salida del worker y muerte de su padre. No
  promete recuperación instantánea si se mata simultáneamente también al guardián,
  falla el compositor o el hardware ya no responde. El timeout del host sigue en 45 s.
- Los incrementos posteriores añadieron [gestos](directional-gestures.md) y
  [atajos personalizados](custom-shortcuts.md), con aceptación registrada por función.
- El paquete Arch se actualizó para incluir la biblioteca, pero no se instaló.

## Construcción y referencias

`src-tauri/build.rs` compila `native/keyboard.c` con el compilador C y pkg-config
para wayland-client/xkbcommon antes de Tauri. Genera
`bridge/libscrollmaster-keyboard.so`, ignorada por Git e incluida como recurso de build.
No hay descarga de crates o paquetes nueva para este incremento.

Protocolo consultado: [virtual-keyboard-unstable-v1](https://github.com/atx/wtype/blob/master/protocol/virtual-keyboard-unstable-v1.xml).
La biblioteca C es propia; el archivo XML y el código de wtype no se incorporan
al repositorio. La implementación HID++ se contrastó con el paquete local Solaar
1.1.20, en `hidpp20.py`, `base.py`, `listener.py` y `notifications.py`.

## Incremento posterior

El motor ahora también ejecuta Copiar/Pegar/Cortar/Deshacer mediante pulsaciones
completas, con catálogo cerrado y perfiles schema 4. La suite vigente suma 99;
las 81 de este registro corresponden al cierre anterior. La recuperación del
guardián se conserva. Ver [button-shortcuts.md](button-shortcuts.md).
