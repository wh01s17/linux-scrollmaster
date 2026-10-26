# N5: captura residente de perfiles con recuperación

Incremento del 21 de octubre de 2026, America/Santiago. El modo experimental
**`--serve-guarded-capture`** conecta perfiles de software con el motor Rust,
salida preparada, consumidor residente y restauración en el proceso independiente.
La GUI la usa con `SCROLLMASTER_BACKEND=native` ([integración](native-gui.md)); por
defecto sigue Python/Solaar. La aceptación física de esta ruta está pendiente:
las pruebas nuevas usan transporte y salida simulados, procesos reales y perfiles
o leases temporales. No se abrió el mouse ni el compositor real.

## Selección y alcance

`WorkerConfig::with_capture` o el nuevo argumento de arranque seleccionan captura;
no se puede cambiar de backend por una petición JSON. Se exige slot 1..6 e
identidad Unit ID mayúscula. El hijo usa la rama privada
`--profile-guardian-capture-child` y el canal autenticado heredado. Los modos
`--serve` y `--serve-guarded` conservan el rechazo de software antes de adquisición.
Construir el proxy y gestionar perfiles locales sigue sin adquirir HID ni teclado.

**Esta opción puede interceptar botones y emitir acciones.** Un restore/remap de
software o la reconciliación de un perfil ya armado durante snapshot puede activar
captura. El mero arranque no arma perfiles ni ejecuta solicitudes de hardware.
No se deben lanzar controladores o escritores concurrentes.

Conserva el contrato persistido de modificadores, combinaciones, gestos y acciones
Hyprland; los controles capturados son únicamente los asignados a software. Puede
aplicar también ajustes y remapeos de firmware en el mismo lote, tras validar todo
el lote. Los remapeos nativos son persistentes; la limpieza solo retira reporting temporal.

El mismo plan se puede confirmar sin repetir setters de captura y los ajustes
escalares pueden cambiar mientras sigue activo. Desde el incremento de transiciones,
un plan de botones distinto se aplica **bajo el mismo dueño** (ver abajo). El
servicio envía el perfil efectivo según el foco en cada snapshot, así que un
cambio de foco es una transición. Los botones se combinan por botón: los ausentes
de la petición conservan su asignación viva, también la de software.

Reconexión: tras un fallo con limpieza confirmada, la **siguiente** petición
reconecta con sesión y preflight nuevos; la fallida no se repite. Una limpieza no
confirmada sigue bloqueando el dueño. El proxy padre conserva el mismo hijo ante
un error estructurado (el hijo es la autoridad de su limpieza) y solo considera
terminal un resultado IPC desconocido.

## Transiciones de plan bajo el mismo dueño

Un objeto `buttons` diferente del plan activo es una transición. Preflight valida
todo el lote sobre el snapshot actual y abre la salida del plan nuevo en estado
cero sin conectarla ni escribir. Apply consume el mismo ticket y sigue este orden:

1. `ProfileEvents::disarm` detiene la entrega y olvida el binding armado.
2. Se cierra la salida anterior (libera modificadores mantenidos).
3. `ReportingRecovery::release_live` limpia solo los bits temporales registrados
   mediante la sesión viva y su consumidor único, con ACK exacto y relectura sin
   caché; no reabre hidraw ni cierra el descriptor. Se exige `completed`.
4. Solo entonces se retiran registro y plan activo, y se aplican escalares y
   remapeos nativos (los remapeos requieren reporting en cero).
5. Si el plan nuevo captura controles: nuevo registro de recuperación con getters
   frescos, salida preflight conectada todavía bloqueada, activación y armado
   con la misma confirmación del lote que una activación inicial.

Un plan sin controles de software termina con captura inactiva y sin registro.
Una liberación no confirmada es terminal: no se envían ajustes, remapeos ni
setters de reemplazo; el registro anterior se conserva y la limpieza final
relee y restaura con el descriptor retenido, sin repetir a ciegas la escritura.
Una preparación descartada o reemplazada cierra solo la salida de reemplazo y
mantiene la captura activa. `native_capture.button_transitions_supported` es
`true` y `last_release` conserva la evidencia de la última liberación en vivo.

## Responsabilidades

| Módulo | Responsabilidad |
|---|---|
| `native/profile_capture.rs` | Preflight completo, ticket local, activación, transición de plan y limpieza del backend guardado |
| `native/profile_events.rs` | Historial acotado, compuerta de salida, sincronización, validación y desarmado de binding activo |
| `native/recovery.rs` | Restauración con descriptor retenido y liberación en vivo compartiendo la misma limpieza |
| `native/profile_backend.rs` | Mismo lector para getters/setters nativos y hooks de salida durante consultas |
| `native/profile_connector.rs` | Admisión explícita y construcción dentro del hijo bajo lease |
| `native/profile_owner.rs` | Cerrar backend antes de liberar lease y conservar errores de recuperación |
| `native/profile_guardian_server.rs` | Cierre confirmado; devolver error si la limpieza no se confirmó |
| `native/profile_guardian.rs`, `worker.rs` | Selección por arranque y proxy; conservar incertidumbre al perder el canal |

## Preparación y activación

La admisión del servicio, dueño y RPC se consulta por backend; solo el conector
de captura permite software. `Changes` conserva la validación del documento y
`ProfileButtonPlan` separa valores nativos de acciones. Preparar exige lecturas
frescas de identidad, layout, capacidades, opciones, DPI y ausencia de interceptación.
Registra `ReportingRecovery`, abre salida en estado cero y hace preflight de
teclado/escritorio antes del desarmado persistido. Todavía no habilita reporting.

Apply consume el ticket del mismo lote, sesión y plazo. Revalida capacidades y
el registro antes de cualquier setter. Aplica valores nativos y luego los setters
cerrados de captura ya existentes: `0x03` para 83/86 y `0x33` para 195, sin remap
ni bits persistentes. Cada escritura exige ACK exacto, lectura posterior y
confirmación final. Un efecto incierto no se reintenta.

La salida permanece bloqueada durante preparación/activación. Se reinicia la
sincronización antes de cada setter: solo un estado posterior al último puede
sembrar botones mantenidos, suprimiéndolos hasta soltarlos. El historial previo
no produce acciones; si no llega un estado, el primer informe vivo sincroniza.
Los valores nativos solicitados deben coincidir en el snapshot final de activación
antes de armar salida. La lectura final del lote verifica también el recibo completo.

El consumidor único entrega eventos tipados durante consultas y reposo. Conserva
64 eventos recientes y un presupuesto de 4096 informes por operación; no hereda
el límite vital de 30 segundos del observador finito. Durante getters, los gestos
físicos conservan recorrido provisional y retienen hasta 64 acciones hasta
confirmar el snapshot con los mismos DPI. Cambiar o perder DPI descarta esas
acciones; una lectura idéntica ya no cancela la pulsación. Ver
[corrección del sondeo](gesture-polling.md). Cambios de layout, mappings o reporting de una
captura activa son terminales. `native_events` identifica entrega al motor;
`native_capture` declara activación/registro y que faltan transiciones.

## Limpieza e incertidumbre

El descriptor original se conserva en un contenedor privado. Solo existe un token
lector no clonable en la sesión activa; un fallo de sesión retira ese token, sin
perder el descriptor necesario para restaurar. No se duplica ni se reabre hidraw.

El cierre libera primero la salida, destruye la sesión activa, transfiere el
descriptor a la sesión de recuperación y finalmente libera el lease. Un error al
liberar teclas no impide restaurar el mouse. La restauración ignora la cancelación
del cliente, valida de nuevo identidad/layout y solo limpia bits temporales
registrados, preservando mappings y flags ajenos. Tiene presupuesto propio de
20 segundos. Drop conserva este camino para EOF, errores y desenrollado por panic.

Shutdown responde `stopped` solo tras cierre confirmado y destrucción del backend.
Un error de salida/restauración queda memorizado: otro close no lo transforma en
éxito ni se reconecta ese dueño. El proxy tampoco reinicia automáticamente una
captura tras apply incierto o pérdida de comunicación; la confirmación de limpieza
queda pendiente y debe comprobarse el dispositivo antes de iniciar otro dueño.
El proceso independiente sigue limpiando aunque el cliente desaparezca.

No hay garantía frente a muerte del propio guardián, pérdida de energía, dispositivo
inaccesible o cambio de identidad. No existe journal persistente ni recuperación
tras reinicio; esos casos no se presentan como restaurados.

## Evidencia y siguientes pasos

30 entradas nuevas: 28 de biblioteca (incluidos dos harnesses de procesos) y dos
integraciones del ejecutable. Cubren captura mixta, modificadores/gestos, memoria,
FIFO anterior, sincronización posterior al setter, tickets/plazos, ACK incierto,
salida/escritorio no disponibles, EOF/cancelación, SIGKILL del cliente, cierre
idempotente, errores persistentes y rechazo de un plan distinto. Los guardianes
de producción pasan handshake, plazo inválido y shutdown sin solicitar hardware;
las operaciones locales del nuevo modo permanecen offline.

**750 pruebas aprobadas**: 178 Python, 41 TypeScript y 531 Rust (447 biblioteca,
20 host, 50 integración y catorce CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets` y
`cargo test --offline --manifest-path src-tauri/Cargo.toml --all-targets`.
Las pruebas de sockets requieren salir del sandbox tras errores de permisos.
También se comprueban formato, diff y máximo de 1000 líneas por archivo de código.

Transiciones (mismo día): 5 entradas nuevas de biblioteca y una sustituida.
Cubren reemplazo software→software con modificador mantenido, software→solo
nativo, liberación perdida sin reintento ni setters posteriores, preparación de
transición descartada y `release_live` sin consumidor/reutilizable. Rust: **535
aprobadas** (451 biblioteca, 20 host, 50 integración y 14 CLI); pasan `pnpm test`,
`pnpm build`, cargo check/test offline `--all-targets`, formato y diff. Sin
hardware, compositor ni perfiles reales; ninguna transición está aceptada físicamente.

Siguiente: transiciones disparadas por foco/perfil desde el servicio duradero,
reconexión de captura sin reintentos inciertos, pruebas físicas optativas con
restauración y paridad Tauri. N5 sigue parcial y N6/retirada de Solaar/Python pendientes.
