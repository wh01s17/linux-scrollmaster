# N4: captura nativa optativa con confirmación y recuperación

Incremento del 21 de octubre de 2026, America/Santiago. `native/activation.rs`
implementa activación temporal de diversion/raw XY, conectada al consumidor,
motor de acciones y guardián independientes. Es una herramienta experimental
finita: **no sustituye el backend de la GUI y no tiene aceptación física**.
Las pruebas usan procesos reales con mouse y salida de teclado simulados. No se
ejecutó `--capture` contra hardware ni se emitieron teclas en el escritorio real.

## Selección explícita

| Modo | Comportamiento |
|---|---|
| `--observe` | Observación sin asignaciones ni activación de reporting |
| `--observe --keyboard-check` | Motor con asignaciones vacías; apertura/cierre de teclado en estado cero |
| `--capture --bindings-json OBJETO` | Prepara salida, activa solo CIDs asignados, confirma y ejecuta acciones hasta terminar |

La API `GuardianConfig::with_capture_actions` valida el catálogo cerrado de
`ActionBindings` y exige coincidencia exacta entre CIDs asignados y registrados
para recuperación. Un objeto vacío falla. La configuración se copia y queda fija
durante toda la sesión; elegir después `with_keyboard_check` elimina la captura.
`ActionBindings::controls` enumera los controles necesarios sin acceder al mouse.

La CLI limita el JSON inicial a 8192 bytes antes de parsearlo. No acepta asignaciones
en observación ni combina captura con keyboard-check. Laterales: Alt/Ctrl, edición,
multimedia, atajos canónicos y las cuatro [acciones de escritorio](native-desktop.md);
195: gestos con cinco direcciones. Comandos y CIDs primarios se rechazan.
El hijo vuelve a validar el modo,
el conjunto registrado y la configuración. No se pasa una ruta de archivo ni se
invoca un shell para interpretar asignaciones. El IPC sigue siendo estado/refresh/cierre.

Formato para una aceptación futura, **no ejecutado**; sustituir interfaz, slot y
Unit ID por un descubrimiento actual, con la GUI y otros controladores cerrados:

```sh
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_guardian -- --capture hidrawN --slot SLOT --device UNITID --seconds 5 --bindings-json '{"83":"modifier:ctrl","86":"shortcut:copy"}'
```

Esta operación sí modifica temporalmente reporting y puede producir entrada en
la aplicación enfocada. Usa permisos hidraw existentes como usuario normal; no
instala servicios ni modifica permisos, perfiles o configuración del escritorio.

## Preparación y confirmación

1. El guardián adquiere el lease y abre hidraw. Registra recuperación mediante
   getters que confirman identidad, layout y reporting cero. Conserva esa propiedad
   hasta acabar la restauración, incluso si muere el cliente.
2. Crea un teclado autorizado y el motor con DPI de la lectura de registro. El
   propietario vuelve a consultar y valida capacidades/DPI antes de instalar su
   único consumidor. La FIFO de ese setup solo se conserva como evidencia.
   Si hay acciones de escritorio, exige preflight Hyprland/Lua antes de activar.
3. Se bloquea la entrega de acciones durante toda la activación. El consumidor
   continúa decodificando y registrando notificaciones intercaladas. No puede
   activar un propietario sin salida preparada o que ya procesó eventos vivos.
4. Antes de **cada** setter se releen identidad, catálogo, capacidades, mapping y
   reporting; se comparan con el estado esperado, incluidos controles previamente
   confirmados. Otra lectura del CID inmediatamente antes de escribir detecta
   cambios de flags/mapping. Un baseline ya interceptado o con flags desconocidos falla.
5. Solo se envía `setControlReporting` temporal: máscara `0x03` para 83/86,
   `0x33` para 195. Remap y bits de cambio ajenos permanecen cero. Se exige eco
   exacto del ACK y getter sin caché: mapping conservado, flags `0x01` o `0x11`.
   No se modifican flags persistentes ni se reintenta un setter incierto.
6. Una consulta completa final confirma el conjunto, identidad y layout. El motor
   acepta ese snapshot/DPI. Solo se puede sembrar sincronización con una lista de
   botones recibida después del último setter; el historial de setup/preflight no
   sirve para ello. Si no existe esa lista, el primer estado posterior sincroniza
   sin acciones y puede suprimir la primera pulsación hasta soltarla.
7. Se habilita la entrega y se anuncia `ready`. El cliente exige modo de captura,
   confirmaciones de todos los CIDs, consumidor armado y asignaciones idénticas a
   las solicitadas. No acepta como sustituto un ACK de observación pasiva.

Los eventos durante confirmaciones llevan `during_activation:true` y
`delivered_to_sink:false`; no se reproducen después para ejecutar acciones.
La sincronización solo consume estado para suprimir botones mantenidos.

## Evidencia, plazos y terminación

El propietario usa `capture-owner` y `read_only:false` desde el intento de
activación. `activation` conserva `completed`, `hardware_confirmed`,
`setters_attempted`, `confirmed`, `attempted`, `pending` y `error`. Las confirmaciones
parciales son lecturas anteriores; no prueban que el estado siga igual si falla la
consulta final. El contador de intentos es conservador y puede incluir una petición
cancelada antes de enviarse. Un setter sin confirmar nunca se presenta como éxito.

`diversion_enabled_by_guardian` indica si alguna activación fue confirmada: `false`
sin intentos, `true` con alguna confirmación o `null` con resultado incierto sin
confirmaciones. `diversion_may_have_been_enabled` distingue los intentos. Son
evidencia histórica; el resultado de `recovery` acredita la restauración terminal.
`capture_armed` indica que la salida llegó a habilitarse, no que siga activa tras cerrar.

La activación tiene hasta 20 s, limitada además por el intervalo de observación:
el presupuesto de 1–30 s empieza al adjuntar el consumidor e incluye activación.
La espera de arranque del cliente es de 65 s para captura de teclado, 75 s si incluye
escritorio y 45 s para observación,
por dos consultas iniciales de hasta 20 s, teclado y activación. Los getters y
setters conservan sus plazos por petición y los límites totales de 128 eventos y
4096 informes. La salida Wayland no amplía el plazo de sesión que la contiene.
Una activación agotada no cuenta como observación completada.

Ante cierre, EOF, error, panic recuperable, plazo o saturación se cierra el motor,
termina el préstamo del lector y se recoge el mismo resultado de liberación del
teclado. Después se restaura reporting con el transporte original y lease retenido.
Una activación parcial restaura todos los CIDs registrados, incluidos los inciertos;
conserva mapping actual y flags ajenos. El cliente no mata al recuperador por timeout.
Si cambió identidad/layout o el dispositivo no responde, la recuperación puede
quedar sin confirmar: se informa el fallo y no se escribe sobre otro dispositivo.
No hay journal durable, recuperación al reconectar ni garantía ante pérdida de
energía o muerte del propio guardián.

## Pruebas y pendiente

34 casos nuevos: 21 de activación/sesión, cinco de procesos del guardián, dos de
configuración y seis del parser de la CLI. Cubren vectores fijos, lectura posterior,
preservación de mapping, gating, estado inicial antiguo, DPI perdido, capacidades
cambiadas entre setters, lote parcial, ACK incorrecto/tardío, ausencia de efecto,
cancelación tras escritura, saturación, salida ausente, repetición rechazada,
SIGKILL durante activación/con modificadores mantenidos y fallos de salida/cierre.

Al introducir captura: **486 aprobadas**: 177 Python, 41 TypeScript y 268 Rust (204 biblioteca, 23 host,
35 integración y seis CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml --all-targets`, formato
y diff. Los sockets privados de las pruebas de procesos requieren salir del sandbox.

Falta aceptación física optativa del ciclo captura/acciones/cierre/fallo; no se
hereda la aceptación del backend Python. La [salida nativa de escritorio](native-desktop.md)
ya está implementada con pruebas sintéticas. El [mapa de botones de perfiles](native-profile-actions.md) ya se compila para
captura finita con remapeos nativos como requisitos previos. Faltan servicio
duradero, reconexión, integración completa perfiles/Tauri y retirada de Solaar/Python. N4 sigue
parcial; el backend experimental no se activa por defecto en la aplicación.
