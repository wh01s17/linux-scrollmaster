# N5: eventos pasivos en el propietario duradero

Actualización: [captura residente](native-profile-capture.md) reutiliza este consumidor
con salida preparada y recuperación en el modo separado `--serve-guarded-capture`.
Este documento conserva el contrato pasivo de `--serve-guarded`.

Incremento del 21 de octubre de 2026, America/Santiago. El backend nativo de
perfiles instala un consumidor después de validar identidad/catálogo y conserva
ese mismo lector durante consultas, confirmaciones y reposo. El guardián de
`--serve-guarded` ya lo sondea entre peticiones. **No habilita diversion ni
acciones de software; motor/salida y ReportingRecovery duraderos siguen
pendientes. La GUI conserva Solaar/Python.** Toda la evidencia nueva es sintética:
no se abrió mouse, compositor ni perfiles del usuario.

## Responsabilidades

| Código | Responsabilidad |
|---|---|
| `native/profile_events.rs` | Decoder compartido, cancelación, contadores y anillo de evidencia pasiva |
| `native/profile_backend.rs` | Instalar consumidor en la sesión validada, marcar fases y detectar cambios de layout |
| `native/profile_owner.rs` | Sondeo sin adquirir/reconectar y cierre de sesión antes de soltar lease ante error |
| `native/profile_connector.rs` | Cancelación del cliente durante inicialización y consumo del dueño guardado |
| `native/profile_guardian_server.rs` | Alternar IPC/eventos en un solo hilo, preservar framing y reportar errores idle en orden |
| `hidpp/session.rs` | Invalidación terminal explícita ante rechazo de identidad/layout por la capa de features |

`NativeProfileBackend::new` sigue construyendo sin E/S. Un poll previo al primer
snapshot no lee ni valida hardware: devuelve false. La primera consulta completa
valida modelo, Unit ID, feature/capacidades y crea el decoder. Solo entonces se
instala el consumidor. Los informes del FIFO inicial se entregan en orden,
marcados como initialization y queued_before_attachment; no producen acciones ni
se reutilizan como sincronización para una captura futura.

El decoder mantiene las validaciones de slot, feature, software ID, CIDs y raw XY.
El primer estado de botones sincroniza sin flancos de pulsación. Movimiento sin
un origen raw conocido se ignora. Un CID inválido cierra el flujo. Para perfiles
solo nativos se admite un catálogo sin controles divertibles: no se inventa esa
capacidad ni se bloquean lecturas/remapeos que antes eran válidos. El decoder de
captura finita sigue exigiendo controles divertibles.

## Consultas, memoria y evidencia

Una vez adjunto, Session entrega informes no correlacionados al consumidor durante
getters, ACK y confirmaciones, sin llenar de nuevo su FIFO de 128. Los ACK y
respuestas correlacionadas siguen perteneciendo al protocolo. No hay descriptor
duplicado, lector de fondo ni cambio de consumidor entre operaciones.

Se comprueba el layout completo tras cada consulta aceptada: feature, versión,
índice, cantidad de controles, capacidades y opciones. Una discrepancia invalida
la sesión antes de devolver éxito. Los destinos nativos y reporting no forman
parte del layout del decoder; sus garantías de preflight/readback permanecen en
el backend. Los remapeos nativos confirmados siguen usando máscara cero para
reporting. Un error durante apply cierra la sesión y conserva confirmed/attempted;
si el firmware cambió antes del fallo del consumidor, no se repite ni se supone
rollback.

`device.native_events` y `event_status()` ofrecen evidencia local, sin E/S:

- initialized y active describen consumidor/sesión en ese instante; no son una
  consulta adicional ni una garantía de presencia futura del mouse.
- reports/events/ignored son contadores acumulados por sesión.
- recent conserva como máximo **64 eventos**, con sequence y phase
  (initialization/snapshot/prepare/apply/idle). history_evicted cuantifica los
  eventos retirados: no se promete un historial completo.
- error conserva el primer fallo del flujo; actions_enabled y
  diversion_enabled_by_backend permanecen false. Este último informa lo que
  hizo este backend, no que los flags físicos estén necesariamente a cero.

La memoria del registro no crece con la duración de la sesión. Cada operación
tras adjuntar el consumidor admite hasta **4096 informes no correlacionados**;
alcanzar el presupuesto detiene la sesión. El presupuesto se renueva por operación
o turno idle, no se aplica como un límite de vida que agotaría un servicio largo.
Un error es terminal para esa sesión y no se borra al empezar otro turno. Una
conexión nueva comienza contadores/historial nuevos.

## Reposo, IPC y cancelación

El servidor comprueba el canal antes de cada turno de hasta **50 ms**. Si hay
hardware adquirido, el dueño revalida controladores y sondea su misma sesión;
si no hay conexión, no descubre, toma lease ni lee HID. Un turno quieto espera
solo el resto de su intervalo en el canal. Los informes continuos no impiden
atender snapshot/shutdown. El sondeo no invalida por sí mismo un ticket preparado;
apply conserva lote, sesión y vencimiento originales.

Una trama parcial mantiene un único plazo de 30 s mientras continúa la recepción
pasiva. No se añade un temporizador de reconciliación, consulta de foco autónoma,
socket2 ni respuesta espontánea. Con `--serve` el consumidor atiende consultas;
el sondeo idle automático pertenece a `--serve-guarded`.

Ante error idle se cierra primero el lector y después el lease, y se invalida el
ticket. El servidor guarda ese error y lo devuelve una sola vez en la siguiente
petición, con su ID esperado y sin ejecutar esa operación ni reconectar. Una
petición posterior puede abrir una sesión nueva. Shutdown conserva su cierre
confirmado aunque hubiera un error pendiente.

El conector guardado comparte la comprobación del peer IPC con la inicialización
y el consumidor. EOF/cierre del cliente se consulta antes de E/S y entre informes;
las esperas de solicitudes quietas ya se dividen en slices. Esto permite detener
la operación en curso antes del siguiente getter/setter. No hace atómica la
comprobación con la escritura ni interrumpe una syscall bloqueada: un efecto ya
en curso puede ocurrir. La limpieza actual cierra sesión/lease; no restaura
reporting porque este dueño todavía no lo activa.

## Verificación y continuidad

27 pruebas nuevas: ocho del consumidor, trece del backend real sobre frames
escritos a mano, dos de política de dueño y cuatro de procesos del guardián con
backend sintético. Cubren 5000 turnos sin agotar la vida de la sesión, historial
acotado, 300 informes durante consulta, saturación por operación, filtros/XY,
inicialización, capacidades solo nativas, deriva de layout, cancelación por EOF,
error después de un remapeo efectivo sin reintento, lease/tickets y equidad IPC.
Las pruebas de procesos acreditan planificación y errores del servidor; los frames
acreditan el consumidor/backend. Ninguna acredita recepción física nueva.

**720 aprobadas**: 178 Python, 41 TypeScript y 501 Rust (419 biblioteca, 20 host,
48 integración y catorce CLI). Pasan pnpm test/build, cargo check/test offline
--all-targets, formato y diff. Los sockets sintéticos se probaron fuera del
sandbox tras errores de permisos dentro. Los 137 archivos de código propios
cumplen el límite de 1000 líneas; mayor: 971, sin excepciones nuevas.

Siguiente: salida preparada y motor de acciones, con registro/restauración de
reporting bajo el mismo dueño independiente antes de activar software. Después,
transiciones por perfil/foco y DPI, reconexión de captura y paridad Tauri/N6.
La herramienta de captura finita sigue separada; no ejecutar dos propietarios
concurrentemente. La aceptación física nativa continúa optativa y pendiente.
