# N5: propietario de perfiles en proceso independiente

Actualización: el modo separado [captura residente](native-profile-capture.md),
`--serve-guarded-capture`, permite un plan fijo de software con recuperación.
`--serve-guarded` conserva el alcance nativo descrito aquí. El servidor ahora
consulta la admisión del backend y exige close confirmado antes de `stopped`.

Incremento del 21 de octubre de 2026, America/Santiago. El servicio de perfiles
puede usar `ProfileGuardianOwner` como proxy de un dueño HID en proceso separado.
El modo experimental `--serve-guarded` lo conecta al worker Rust. **N5 sigue
parcial; la GUI conserva Python/Solaar y el nuevo dueño sigue rechazando acciones
de software.** No se abrió hardware, compositor ni perfiles reales durante este
incremento. La evidencia utiliza procesos reales, leases/perfiles temporales y
backend sintético; no acredita captura ni recuperación física.

## Propiedad y selección

| Código | Responsabilidad |
|---|---|
| `native/profile_guardian.rs` | Proxy `ProfileBackend`, lanzamiento diferido, preparación local, cierre y recogida del hijo |
| `native/profile_guardian_server.rs` | Protocolo privado serial, preparación remota de un solo uso y destrucción del backend antes de confirmar cierre |
| `native/profile_guardian_wire.rs` | Mensajes tipados, límites y plazo absoluto entre procesos |
| `native/guardian.rs` | Socket autenticado y lanzamiento independiente compartidos con el guardián finito |
| `worker.rs`, `bin/scrollmaster-native-worker.rs` | Modo optativo y rama interna del hijo; el contrato público no recibe comandos/rutas |
| `service.rs` | Cierre explícito previo al acuse de shutdown, además del desarmado persistido existente |

`WorkerConfig::with_guardian` o `--serve-guarded --slot 1..6 --device UNITID`
seleccionan esta ruta. `--serve` mantiene el dueño anterior; no hay selección por
IPC, cambio de preferencias, fallback o activación desde Tauri. Ambos modos
pueden aplicar ajustes/remapeos y guardar perfiles; `snapshot` puede reconciliar
un perfil armado. No se ejecutan concurrentemente con otro controlador/escritor.

Construir el proxy no inicia procesos, toma lease, descubre dispositivos ni
abre perfiles. Las operaciones locales del servicio permanecen offline. El
primer snapshot/preflight de hardware inicia el mismo ejecutable con argumentos
cerrados `--profile-guardian-child --slot ... --device ...`; el hijo valida la
selección y el canal antes de alojar `NativeProfileOwner`. El handshake solo
confirma protocolo/identidad configurada, no presencia o identidad física.
La primera lectura sigue haciendo el handshake HID de ese dueño.

El padre conserva la única escritura de perfiles. El hijo conserva el único
descriptor/lector y `HardwareLease`, con las políticas existentes de controles
concurrentes, una interfaz Bolt sin ambigüedad, Unit ID y reconexión por petición.
No hereda HID ni un lease del padre. El canal Unix privado usa fd 3, credenciales
del padre/usuario y CLOEXEC; el hijo tiene su propia sesión de procesos. Cerrar o
matar al cliente produce EOF sin matar automáticamente al propietario.

## Protocolo y presupuesto

El canal admite exclusivamente snapshot, prepare, apply y shutdown. No acepta
rutas, ejecutables, bytes HID, foco ni documentos de perfiles. Cada petición lleva
un ID consecutivo. Los structs Serde rechazan campos desconocidos o repetidos en
el objeto exterior; los cambios anidados conservan el contrato de validación de
perfiles y no añaden detección de duplicados anidados.

Prepare valida el conjunto nativo antes de adquirir hardware y devuelve un ticket.
El proxy conserva cambios, plazo e ID; el hijo conserva los mismos cambios y su
plazo. Apply exige el ticket vigente y lo consume antes de validar/escribir.
No admite un nuevo lote o presupuesto. Un snapshot, otro preflight, parámetros
distintos, fallo o cierre invalidan la preparación. Se conserva el orden del
servicio: preflight, publicación confirmada del desarmado, apply en la misma sesión.
Se contrastan identidad y valores finales también a ambos lados del canal.

El presupuesto máximo de **30 s** incluye arranque del hijo, envío, consultas y
respuesta. Se transmite el vencimiento absoluto de CLOCK_MONOTONIC, con conversión
conservadora respecto a Instant; el tránsito o la espera de disco no conceden
un presupuesto nuevo. El apply remoto conserva exactamente el Instant usado en
su preflight. El padre descarta respuestas tardías y nunca repite un apply incierto.
Los errores parciales conservan confirmed/attempted; un fallo del canal durante
apply informa resultado incierto sin inventar confirmaciones.

Las peticiones admiten **8192 bytes sin LF**, y las respuestas hasta **2 MiB
incluyendo LF**. Framing/JSON/UTF-8 inválido, exceso, secuencia incorrecta o EOF
parcial cierran el hijo antes de otra operación. Un intervalo sin datos puede
continuar indefinidamente; una trama parcial no renueva el plazo de recepción
de 30 s. Una respuesta que no cabe o no se puede enviar termina el propietario;
puede haber efectos ya realizados. No se añade un límite duro a toda la memoria.

## Cierre y fallos

Timeout, contrato inválido o fallo de canal cierran el IPC y conservan el handle
del hijo. El proxy no inicia otro mientras ese hijo siga cerrando; una petición
posterior solo puede iniciarlo tras recoger su salida. Entre workers distintos
el lease cooperativo sigue impidiendo adquirir simultáneamente el hardware.
No se mata al hijo ni se reenvía la petición. Drop recoge al hijo en un hilo
separado; tras muerte del padre, su adopción/recogida corresponde al sistema.

Shutdown destruye el backend del hijo antes de devolver stopped. El proxy además
recoge el proceso dentro del plazo. `ProfileService::shutdown` y el worker exigen
esa confirmación antes de responder `stopped:true`. Si agota el plazo, devuelven
un error de etapa shutdown y dejan terminar el cierre. EOF/panic/salida fallida
siguen utilizando Drop; esa vía no confirma sincrónicamente que el hijo terminó.

El [consumidor duradero](native-profile-events.md) ya comprueba la pérdida del
cliente antes de E/S y entre informes, también durante inicialización. Esto no
interrumpe una syscall bloqueada ni impide un efecto entre comprobación y escritura;
no hay garantía de tiempo real.
Este dueño no activa diversion, por lo que su limpieza actual es cerrar la sesión
antes de liberar lease. **El consumidor pasivo ya está integrado; motor, teclado
y ReportingRecovery duraderos siguen pendientes.** La captura finita de [botones de perfiles](native-profile-actions.md)
sigue siendo una herramienta separada; no ejecutar ambos propietarios juntos.

## Evidencia y continuidad

25 entradas nuevas de suite: 21 de biblioteca (incluidos dos harnesses de
subprocesos) y cuatro de integración. Cubren construcción offline y software
rechazado antes de adquisición; proceso persistente; tickets y deadlines;
desarmado de disco antes de escritura remota; error parcial/recibo incorrecto;
timeout sin reintento; lease retenido durante cierre; SIGKILL del cliente;
framing/duplicados; cierre confirmado y error de shutdown sin éxito falso.

Las cuatro pruebas del ejecutable verifican gestión local, rechazo de software
sin desarmar y rechazo de la rama interna sin canal. La última inicia el guardián
de producción con un socket privado real: handshake, rechazo de software antes
de getters y shutdown, sin solicitar una lectura o escritura física.

**693 aprobadas**: 178 Python, 41 TypeScript y 474 Rust (392 biblioteca, 20 host,
48 integración y catorce CLI). Pasan pnpm test/build, cargo check/test offline
--all-targets, formato y diff. Las suites de sockets sintéticos se ejecutan fuera
de la restricción del sandbox. Los 133 archivos de código propios cumplen el
máximo de 1000 líneas; máximo actual 971, sin nuevas excepciones.

Siguiente: conectar salida y acciones con recuperación en este dueño duradero,
transiciones por perfil/foco, confirmación de DPI y reconexión. Antes de habilitar
software deben existir consumidor único activo, salida preparada y restauración
independiente antes de liberar el lease. Después, paridad Tauri y N6 sin
Solaar/Python. La aceptación física nativa sigue optativa y pendiente.


La [continuación de eventos pasivos](native-profile-events.md) eleva la suite a
720 pruebas e integra el consumidor durante consultas y reposo. La evidencia
anterior de 693 se conserva como corte del protocolo/propietario independiente.
El modo guardado sigue sin habilitar diversion ni software.
