# N4: proceso guardián nativo con propietario exclusivo

Incremento del 20 de octubre de 2026, America/Santiago. `native/guardian.rs` y
el ejemplo `native_guardian` conectan el propietario pasivo y el motor de
restauración en un proceso independiente. El proceso cliente solo tiene IPC:
**el guardián abre y conserva el descriptor hidraw y el lease durante todo el
ciclo**. No hay descriptor HID duplicado, segundo lector ni transferencia de
lectura entre procesos al cerrar. No está conectado a la GUI. La observación no
activa diversion; el modo separado [de captura optativa](native-capture.md) ya admite
asignaciones explícitas, setters confirmados y recuperación, sin aceptación física.
La [salida de teclado](native-keyboard.md) incorpora una comprobación voluntaria de
apertura/cierre en estado cero, sin asignaciones ni pulsaciones.
Ahora prepara el motor con asignaciones vacías y [entrega tipada interna](native-action-delivery.md).
Los fixtures de proceso prueban la salida y su cierre. `--capture --bindings-json`
fija asignaciones al arrancar; el IPC continúa sin permitir configurarlas ni activar reporting.

## Decisión de propiedad

En lugar de heredar un descriptor y recuperarlo después de la muerte de su
lector, el guardián es siempre el propietario del hardware. Comprueba conflictos
con otros controladores, toma el lease común, descubre la interfaz Bolt y valida
su apertura con metadata/ioctl. No instala servicios ni cambia configuración.

El proceso consulta el baseline mediante `ReportingRecovery::prepare`, sin
escrituras. Luego abre `PassiveEventOwner` sobre un préstamo léxico del mismo
transporte. Solo anuncia `ready` después de registrar los CIDs y validar la sesión
pasiva; en captura, además confirma el conjunto y arma la salida antes del ACK.
El cliente verifica versión de protocolo, identidad, modo, registro, asignaciones y
estado abierto antes de aceptar ese ACK.

El primer diagnóstico de registro precede a la observación: su FIFO no se
incorpora al historial de eventos. La captura normal comienza en el propietario
pasivo, que conserva las notificaciones de su propio setup y consultas. No hay
acciones ni diversion propias durante ninguna de las dos consultas iniciales.

Ante cierre, EOF, error de protocolo, fallo del propietario o panic recuperable,
se cierra y destruye el propietario activo. Así termina su préstamo del transporte.
Si se solicitó teclado, se libera y cierra antes de restaurar, incluso si falla
su confirmación de liberación. Después se consume el registro y el transporte en la restauración. El lease se
libera tras esa operación, antes de publicar el informe terminal. Un timeout de
sesión solo cierra el préstamo activo; el transporte original sigue disponible
para la restauración en una sesión nueva con identidad revalidada.

El guardián usa una sesión de proceso propia, sin terminal de control. El cliente
no lo mata al desconectarse o al vencer su espera: cierra el IPC y recoge al hijo
con un reaper, para permitir que termine la restauración. Tras SIGKILL del cliente,
el sistema recoge al hijo huérfano; no se promete que el proceso muerto lo recoja.

## IPC y límites

Solo se hereda un socket Unix anónimo privado, sin nombre, puerto o servicio
accesible. No se heredan el descriptor HID ni el lease. El helper valida que el
socket pertenezca al padre que lo lanzó y al mismo usuario mediante `SO_PEERCRED`.
Los descriptores fuente mantienen CLOEXEC; `pre_exec` solo ejecuta `setsid` y
`dup2`. No modifica flags de descriptores del cliente.

| Mensaje del cliente | Comportamiento |
|---|---|
| `status` | Estado conservado por el propietario; no fuerza getters |
| `refresh`, `timeout_ms` | Nueva consulta con el mismo propietario y consumidor; de 1 a 20000 ms |
| `stop` | Cierre y restauración, seguido de informe terminal y salida |

Cada comando lleva un `id` entero estrictamente consecutivo desde 1. Se rechazan
operaciones desconocidas, campos extra, argumentos fuera de rango, JSON inválido
y secuencias incorrectas. No se aceptan bytes HID, setters, rutas, comandos shell,
acciones o preferencias por IPC. La configuración inicial usa nombre `hidrawN`,
slot, Unit ID, CIDs secundarios únicos y duración. La CLI de observación registra
83/86/195; captura deriva el conjunto exacto de sus asignaciones validadas. La API
permite registrar un subconjunto explícito. El JSON inicial de captura admite
hasta 8192 bytes y viaja como argumento de arranque, separado del IPC de comandos.

Los comandos admiten hasta 256 bytes y las respuestas hasta 128 KiB, en JSON-lines.
Las lecturas conservan fragmentos y mensajes consecutivos, sin lectores auxiliares.
Los sockets no bloqueantes y `poll` aplican plazos absolutos incluso con presión
de escritura o un cliente que deja de leer. Las esperas IPC ordinarias son de 2 s.
Un error de IPC del cliente cierra el canal y deja recuperar al guardián.

El arranque hace dos consultas iniciales, de hasta 20 s cada una; el cliente espera
el ACK hasta 45 s para observación, 65 s para captura de teclado y 75 s con
[acciones de escritorio](native-desktop.md), incluidos sus preflights.
La captura añade hasta 20 s de
activación dentro del intervalo de observación. Con `--keyboard-check`, la confirmación de apertura
de teclado añade hasta 2 s dentro de esa espera. La observación sigue limitada a 30 s, 128 eventos y 4096 informes.
El guardián sondea hasta 50 ms y consulta el estado del canal durante getters:
EOF cancela el trabajo activo antes de restaurar. No hay heartbeat que transfiera
el lector o escriba sobre otro propietario.

`finish` espera hasta 25 s la restauración y salida. No reintenta ni mata al hijo
si falla esa espera. Conserva un informe terminal que llegó antes del siguiente
comando, incluso cuando el envío coincide con el cierre del socket. Los límites
son cooperativos: una syscall de driver retrasada o no interrumpible puede durar
más. Un canal heredado por otro proceso sin exec puede retrasar EOF; el presupuesto
finito de observación sigue cerrando la sesión.

## Evidencia y resultados

El informe final contiene modo `passive-guardian` o `capture-guardian`, trigger, error, último estado
del propietario y evidencia de recuperación. `completed:true` exige cierre por
`stop` o plazo normal y recuperación confirmada. Una recuperación correcta tras
EOF, panic o protocolo inválido conserva el error original y no convierte la
observación en éxito. La evidencia de recuperación distingue confirmaciones,
escrituras confirmadas, intentos inciertos y controles pendientes.

El registro por sí solo no activa botones: el modo de captura exige asignaciones
explícitas, salida preparada y confirmación. Aunque el baseline exige flags cero, la restauración puede limpiar
flags temporales registrados que aparecieron después; por eso la herramienta no
se presenta como estrictamente de solo lectura. Conserva mapping y flags ajenos,
según el [contrato de restauración](native-recovery.md).

Quince casos nuevos verifican diez escenarios de proceso y cinco de configuración
e IPC. Dos entradas auxiliares del harness ejecutan los subprocesos sintéticos;
las 17 entradas Rust nuevas elevaron la suite de aquel incremento a 393.
Con captura confirmada, la suite vigente tiene **486 aprobadas**:
268 Rust (204 biblioteca, 23 host, 35 integración, seis CLI) y 218 Python/TypeScript.
Pasan `pnpm test`, `pnpm build`, check de todos los targets, suite Rust completa,
formato y diff. Las pruebas de sockets Unix requieren salir del sandbox.

Los escenarios incluyen estado/refresh/cierre y recogida del hijo; sesión de
proceso independiente; registro rechazado; timeout al arrancar; EOF con lease
retenido durante restauración; SIGKILL del cliente en reposo y durante un getter;
mensajes inválidos/grandes; fin automático anterior al siguiente comando; panic;
desconexión; ACK incorrecto; fragmentación; límites de tramas y presión de escritura.

Usan procesos reales con un mouse sintético y runtime temporal privado. La prueba
simula flags activos en memoria después del setup y comprueba su limpieza por
bytes HID++ y estado final; no habilita flags en hardware. **No hay nueva aceptación
física del guardián, captura activa ni recuperación nativa en Bolt.**

## Herramienta optativa

Ejemplo de aceptación futura, no ejecutado en este incremento:

```sh
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_guardian -- --observe hidraw7 --slot 2 --device <UNIT_ID> --seconds 5 --refresh
```

Exige la GUI y otros controladores cerrados y acceso hidraw existente como usuario
normal. No abre hardware sin argumentos válidos. La selección debe obtenerse de
un descubrimiento actual; no asumir que `hidraw7` o el slot sigan siendo los mismos.
No carga perfiles, no usa Python/Solaar y no cambia permisos o escritorio. El flag
interno `--guardian-child` exige el canal privado heredado; no es una API de red.

## Pendientes

La salida de teclado Wayland en Rust y su limpieza ya tienen pruebas sintéticas.
El [motor de acciones/gestos](native-actions.md) ya tiene [entrega directa interna](native-action-delivery.md).
La [captura optativa](native-capture.md) implementa asignaciones, registro y
confirmaciones. Falta aceptación física de cierre, error, desconexión y crash. Matar también al
guardián, desconectar un dispositivo que no vuelve a responder o apagar el sistema
no garantiza restauración. No hay journal durable ni recuperación al reconectar.

N4 sigue parcial. N5 conectará el servicio nativo con perfiles y Tauri; N6 retirará
Solaar/Python y validará distribución y reglas de acceso. La GUI actual conserva
su backend previo.


La opción `--keyboard-check` (desactivada por defecto) prepara/cierra un teclado
virtual en estado cero y lo incluye en el ACK y la evidencia terminal `keyboard`.
No habilita IPC de pulsaciones ni diversion. La liberación ocurre antes de la
restauración; un error no evita restaurar el mouse y no se declara éxito.
Detalles y límites de confirmación: [teclado nativo](native-keyboard.md).
