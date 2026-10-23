# N5: dueño de perfiles con conexión diferida

Actualización: el conector del modo separado [captura residente](native-profile-capture.md)
permite software. El dueño cierra el backend bajo lease también ante error/Drop;
un fallo de recuperación queda memorizado e impide reconectar ese dueño.

Incremento del 21 de octubre de 2026, America/Santiago. `NativeProfileOwner`
implementa `ProfileBackend` y conecta el servicio de perfiles con una sesión
nativa adquirida bajo lease. **N5 sigue parcial; la GUI utiliza Solaar/Python.**
No se abrió hardware, compositor ni perfiles del usuario durante este incremento.

## Separación y arranque offline

| Código | Responsabilidad |
|---|---|
| `native/profile_owner.rs` | Sesión única, handshake, cierre y preparación de un apply por petición |
| `native/profile_connector.rs` | Procesos, lease compartido y descubrimiento/apertura del sistema |
| `native/profile_backend.rs` | Lecturas completas, preflight y setters con confirmación HID++ |
| `service.rs` | Perfiles, revisión, desarmado persistido y reconciliación |

`NativeProfileOwner::system(slot, identity)` solo valida Bolt slot 1–6 y Unit ID
mayúsculo de ocho hexadecimales. No enumera interfaces, comprueba procesos, toma
lease, crea archivos ni consulta HID. Puede entregarse a `ProfileService::new`
aunque el mouse esté ausente. La ruta del almacén y la identidad se suministran
explícitamente: no se deduce el dispositivo a partir de un slot, nombre o perfil.

Las operaciones locales del servicio siguen disponibles offline y no llaman al
dueño: auto/rename/delete/export/import_preview/import/rule_set/rule_delete.
Solo una operación que necesita hardware puede iniciar conexión. Un snapshot
fallido devuelve dispositivos vacíos y errores, conservando perfiles/activación.

## Adquisición y reconexión

1. Comprueba plazo y ausencia de Solaar/logid, GUI o worker Python anterior.
2. Adquiere `HardwareLease`, compartido con el backend actual, y vuelve a comprobar
   procesos/plazo. No termina otros procesos ni cambia permisos/configuración.
3. Descubre de nuevo interfaces Bolt C548; exige **exactamente una** candidata
   validada. Cero candidatas o varias producen error, sin abrir ninguna. No sondea
   automáticamente otros slots ni elige el primer receptor disponible.
4. `Candidate::open` vuelve a validar sysfs, nodo, descriptor e ioctl, protegiendo
   frente a la reutilización de `hidrawN`. El dueño conserva el backend y el lease.
5. Revalida procesos/plazo y lee un snapshot completo mediante el backend. El
   diagnóstico valida modelo/nombre/Unit ID real; una identidad distinta impide
   acceder al catálogo de botones y no permite setters. Solo entonces contabiliza
   una conexión validada. La primera petición snapshot usa esa misma lectura.

Mientras la sesión responde, las peticiones siguientes reutilizan descriptor,
lector y lease, y consultan de nuevo los valores. Ante error de getter, preflight,
apply, identidad, proceso competidor o plazo, se invalida la preparación y se
destruye el backend **antes** de liberar el lease. También ocurre al destruir el
dueño; `disconnect()` es local e idempotente. No hace restauración de reporting,
porque este dueño no activa diversion ni salida de teclado.

Una petición posterior puede adquirir y descubrir otra vez. No conserva el número
hidraw anterior, por lo que admite renumeración sin seleccionar otro mouse: exige
el mismo Unit ID y slot configurados. No cambia de slot después de un nuevo
emparejamiento. La política con varios receptores sigue pendiente de un selector
estable explícito; hoy se rechaza la ambigüedad. Cada llamada hace como máximo
un intento; no hay temporizador, backoff, hilo ni bucle autónomo de reconexión.

## Escrituras y errores inciertos

`prepare(changes, deadline)` valida primero que las asignaciones sean nativas.
Un perfil de software se rechaza sin adquirir lease ni ejecutar getters. Tras
conexión/handshake, prepara todo el conjunto y conserva una autorización local
para **un** apply de los mismos ajustes/botones, sesión y plazo absoluto.

`apply` consume esa preparación antes de verificar procesos/plazo. No puede
adquirir ni reconectar. Un cambio de parámetros/plazo, lectura intermedia, cierre
o segundo apply exige otro preflight. Esto mantiene la misma sesión entre la
validación del servicio, el desarmado persistido y el primer setter. El backend
también repite su preflight fresco antes de aplicar y confirma todo al terminar.
El dueño vuelve a contrastar identidad/valores del recibo final y el plazo.

Un fallo parcial conserva `confirmed` y `attempted` como evidencia histórica,
cierra y devuelve error; no intenta rollback ni repite la petición. Un snapshot
posterior consulta el estado real. Si sigue armado un perfil automático, el
servicio puede realizar una reconciliación nueva e idempotente con lecturas
frescas; esto no reproduce el intento incierto ni acciones de software.

`status()` solo informa identidad, slot, posesión local de sesión, intentos,
conexiones validadas y último error. `session_owned` no prueba que el mouse siga
presente; la siguiente lectura establece su estado actual. No expone snapshots
cacheados como estado vivo.

El presupuesto absoluto del servicio se comparte en adquisición, descubrimiento,
handshake, preflight, persistencia, setters y confirmación; callbacks tardíos no
producen éxito. Las consultas HID ya tienen plazos. Syscalls de filesystem/procesos
no son cancelables: este componente síncrono no sustituye el aislamiento por
proceso y timeout del futuro worker. Los guards de procesos son cooperativos,
no una exclusión atómica frente a controladores que arranquen después. El lease
no excluye escritores locales que ignoren el protocolo de perfiles.

## Evidencia y continuidad

21 pruebas nuevas, sin dispositivos ni escritorio reales:

- 18 de política: construcción offline; operación local sin probes; servicio con
  archivos temporales; conexión/reconexión; identidad; proceso/lease competidor;
  cierre antes de liberar; lectura fresca; plazo compartido/agotado; preparación
  de un solo uso; recibos falsos/tardíos y fallo parcial sin reintento.
- Tres conectan el dueño con `NativeProfileBackend` real sobre frames HID++
  escritos a mano: fallo/reconexión con catálogo actualizado y un lector máximo;
  Unit ID cambiado antes de consultar botones; fallo de ACK tras DPI confirmado,
  cierre y lectura posterior sin repetir setters. El conector de estas pruebas es
  sintético; no acreditan hotplug ni procesos/lease de un sistema físico real.

**606 aprobadas**: 178 Python, 41 TypeScript y 387 Rust (318 biblioteca, 23 host,
35 integración y once CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml --all-targets`, formato
y diff. Los sockets/procesos existentes se ejecutaron fuera de la restricción
del sandbox. Auditoría: 115 archivos de código propios, ninguno con más de
1000 líneas; máximo 977. No hace falta excepción para este incremento.

El [worker JSON-lines](native-worker.md) ya aloja este dueño en un proceso
separado, con gestión offline probada y cierre por EOF/shutdown.
El [foco/catálogo nativo](native-focus.md) ya entrega al servicio clases observadas,
con fallback al general y pruebas sintéticas.
Siguiente: integrar guardián/salidas con perfiles de software y completar el
contrato del worker. Después,
sustituir el worker Tauri manteniendo un solo lector y escritor de perfiles.
La aceptación física de reconexión/captura/recuperación sigue optativa y pendiente;
N6 retirará Solaar/Python tras paridad e instalación limpia.


El [propietario independiente de perfiles](native-profile-guardian.md) ya conecta
este dueño al servicio mediante `--serve-guarded`, con descriptor/lease en el hijo,
preparación de un solo uso y cierre confirmado. El padre conserva la persistencia.
La ruta es optativa y solo aplica ajustes/remapeos nativos; acciones, consumidor
y restauración duraderas siguen pendientes. La GUI conserva el backend actual.
