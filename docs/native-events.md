# N4: recepción pasiva de notificaciones

Primer incremento N4 del 20 de octubre de 2026. `native/events.rs` añade un
decodificador 1B04 y una observación finita sobre la misma sesión que inicializa
el dispositivo. La herramienta separada `native_events` no habilita reporting,
diversion ni acciones y no está conectada al worker de la GUI. **N4 sigue parcial**:
la biblioteca ya añade [guardián](native-guardian.md), recuperación y
[entrega interna al motor](native-action-delivery.md). La [captura optativa](native-capture.md)
ya implementa asignaciones y confirmaciones, con aceptación física pendiente.

## Contrato implementado

1. Se validan duración y slot antes de E/S. La identidad estable y la feature 1B04
   se obtienen mediante los getters N2/N3, por el mismo propietario del transporte.
   Se reutilizan la selección Bolt validada, el lease y la exclusión de controladores.
   Los informes intercalados permanecen en la cola FIFO de la sesión (máximo 128)
   y se consumen antes de leer nuevos informes. Un desbordamiento cierra el transporte.
2. El índice de feature es dinámico. Solo se interpretan notificaciones del slot
   seleccionado, de ese índice y con software ID cero. Respuestas, errores ajenos,
   otros slots/features y eventos desconocidos se cuentan como ignorados.
3. El evento de botones contiene hasta cuatro CIDs de 16 bits. Se exigen ocho
   bytes, lista compacta sin duplicados y fuentes secundarias físicas, reprogramables
   y divertables anunciadas entre 83/86/195. No se deducen liberaciones de paquetes
   ajenos ni de listas inválidas. Un CID no admitido detiene la observación.
4. La primera lista sincroniza los botones mantenidos y lleva `initial:true`;
   no fabrica flancos de pulsación al comenzar durante una pulsación. Las siguientes
   producen diferencias de conjuntos `pressed`/`released`, aunque cambie el orden.
5. Raw XY exige cuatro bytes, deltas de 16 bits con signo en big endian y algún
   control mantenido con capacidad raw XY anunciada. Sin ese contexto se ignora.
   No se atribuye movimiento a una acción, no se acumulan gestos ni se inyecta entrada.
6. La colección termina por plazo, cancelación, 128 eventos o 4096 informes.
   Usa esperas de como máximo 50 ms y un plazo monotónico; no crea otro lector.
   Los límites y errores conservan evidencia parcial con `completed:false`.
   No se eliminan silenciosamente eventos para continuar una captura saturada.
7. Al terminar se limpia el estado lógico y se devuelve `cleared_held`. Esto no
   libera teclas del escritorio ni restaura firmware: este incremento no los modifica.

`Session::receive` ahora rechaza una lectura exitosa que regresa fuera de su plazo
y cierra ese transporte. Un timeout de espera en reposo sigue siendo normal y
mantiene abierta la sesión. Esta diferencia evita entregar un flanco tardío.

## Herramienta optativa y límites

```sh
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_events -- --observe hidraw7 --slot 2 --device <UNIT_ID> --seconds 1
```

Requiere slot, Unit ID e intervalo explícitos de 1–30 segundos. Sin argumentos o
con argumentos inválidos termina sin acceder al mouse. El hijo supervisado usa un
plazo de 25 segundos más el intervalo elegido y una salida máxima de 128 KiB.
En caso de fallo devuelve `ok:false` con la evidencia disponible. No carga perfiles,
no llama Solaar/Python y no ofrece una ruta genérica para enviar comandos HID++.

La biblioteca recibe un callback de cancelación propio (`FnMut + 'static`); se
consulta antes de inicializar
y durante la colección. La inicialización usa getters con un plazo de 20 segundos
y admite cancelación entre E/S con esperas de hasta 50 ms. Los getters ya preservan los
informes intercalados: `queued_at_start` cuenta los pendientes al comenzar la
colección y `queued_before_observation` distingue sus eventos de los recibidos
durante la escucha. `elapsed_ms` mide el momento de procesamiento desde el comienzo
de la colección, no el instante físico de llegada. No usar ese tiempo para
reconstruir gestos acumulados durante la inicialización.

El transporte ya admite un consumidor síncrono durante las consultas después de
validar identidad/capacidades. Sin consumidor instalado, las consultas conservan
hasta 128 informes y después fallan. El observador usa ese consumidor para su cola
inicial y escucha en reposo; el nuevo [propietario](native-owner.md) permite
consultas posteriores. `--refresh` realiza una consulta adicional con ese propietario.

Esta herramienta **no es el servicio para diversion**: no integra el guardián,
acciones ni un servicio duradero. Esas capas se desarrollan separadamente.
Tampoco hay seguimiento continuo de reconexión/identidad ni recuperación de
teclas/reporting. No hay un segundo lector ni captura activa.

Con reporting normal el firmware puede no enviar ninguno de estos eventos.
Una escucha vacía no acredita botones desviados ni gestos funcionando. No se
debe activar diversion con otra herramienta para producir eventos de prueba.

## Procedencia y pruebas

Layout contrastado con la [especificación Logitech 1B04, eventos 0 y 1](https://lekensteyn.nl/files/logitech/x1b04_specialkeysmsebuttons.html).
Los paquetes de las pruebas son sintéticos, construidos independientemente del
decoder. No se usa una captura real ni Solaar como oráculo de la suite.
La aceptación de revisiones 2–5 conserva la restricción N3; no demuestra que todos
los firmwares de esas revisiones estén verificados.

Quince pruebas nuevas cubren sincronización inicial, flancos, reordenamiento,
filtrado, longitudes, CIDs inválidos, deltas con signo, capacidades, cancelación,
límites, fallos parciales y observación completa con getters solamente. Una prueba
de transporte adicional verifica rechazo/cierre ante una lectura idle tardía.

Suite actual: **486 pruebas aprobadas**: 218 Python/TypeScript y 268 Rust
(204 biblioteca, 23 host, 35 integración, seis CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml` y formato.
La suite regular no accede al mouse. Tres pruebas del host usan sockets Unix
locales: el sandbox los bloqueó y la suite completa pasó al repetirla con ese acceso.

## Evidencia física de escucha en reposo

Prueba de solo lectura del 20 de octubre de 2026, con guard de procesos y lease
aprobados: Bolt `hidraw7`, slot 2, Unit ID <UNIT_ID>. La inicialización confirmó
83→83, 86→86 y 195→195, flags cero; DPI 1800, SmartShift 10, modo 2,
inversión/resolución vertical false e inversión horizontal true.

Resultado: `completed:true`, `stop_reason:"deadline"`, `elapsed_ms:1000`,
cero informes y cero eventos. No se habilitó diversion. Solo se acepta la escucha
en reposo y su salida temporal normal; la decodificación de eventos y la recuperación
no tienen aceptación física en este incremento. El hash del archivo de perfiles
antes y después fue idéntico:
`2d83fccaec0f7632efec9f8e9a5920b0e33a1df72532926d21447fced47a93bd`.
Salida temporal local: `/tmp/scrollmaster-native-events-observe.json`.

Siguiente incremento: construir el guardián e integrar el
[propietario pasivo](native-owner.md) en el servicio, con pruebas de recuperación antes
de habilitar reporting. N3 conserva su aceptación ampliada pendiente; N5/N6 y la
retirada de Solaar/Python de la GUI todavía no se han realizado.


## Continuación N4: conservar informes durante consultas y confirmaciones

Los getters de identidad, capacidades, botones y emparejamientos ya no vacían
la cola después de cada respuesta. La FIFO existente pertenece a la sesión,
conserva el orden entre consultas y mantiene su límite global de 128 informes.
Los ACK correlacionados se pueden recibir con la cola exactamente llena; el
siguiente informe ajeno desborda y cierra el transporte, sin reintento ni reset.
Incluso un desbordamiento durante un getter opcional invalida el diagnóstico
completo. No se publica una lectura parcial como snapshot válido.

`apply_changes_session` y `apply_button_changes_session` son fronteras internas
que toman prestada la sesión existente. Conservan la validación completa previa,
identidad, ACK/readback y errores `confirmed`/`attempted`, con el mismo propietario
durante todo el lote. No habilitan diversion. Los wrappers públicos y las CLI N3
siguen siendo operaciones aisladas: cierran la sesión al terminar y no ofrecen
entrega continua de eventos. Esto prepara la integración futura, no la sustituye.

Siete regresiones sintéticas nuevas verifican eventos entre identidad/getters de
botones, setters escalares y de botones, ACK y confirmación; orden FIFO, clasificación
del backlog, capacidad exacta y desbordamientos antes/durante/después de escribir.
El fallo posterior al ACK mantiene la escritura como incierta: puede haber cambiado
el firmware y no se reenvía. Las pruebas no usan hardware ni activan captura.
No se realizó una nueva prueba física en este incremento; la evidencia anterior
solo acredita escucha en reposo. Guardián, consumo durante las consultas,
aceptación física de eventos siguen pendientes; la cancelación durante el setup
ya está implementada con evidencia sintética.


## Continuación N4: consumidor síncrono en la sesión

`ReportConsumer` pertenece al mismo hilo que el transporte. Se instala una sola
vez con `Session::install_consumer`, después de validar identidad/capacidades.
Primero recibe la FIFO pendiente en orden. Durante `request` recibe cada informe
no correlacionado inmediatamente, sin añadirlo a la FIFO; respuestas y errores
correlacionados siguen perteneciendo al llamador. El consumidor filtra slots,
features y software IDs, porque no todo informe ajeno es una notificación.

`poll_consumer` entrega un informe durante reposo o devuelve false ante silencio.
Usa esperas de hasta 50 ms. `receive` rechaza su uso si hay consumidor instalado;
no puede competir con él. No hay API para sustituir o retirar un consumidor vivo.
El observador pasivo instala su decodificador/registrador acotado en esta vía y
conserva su contrato de salida y sus límites de 128 eventos/4096 informes.
Su callback de cancelación ahora debe ser propio, sin referencias temporales;
puede capturar un indicador compartido mediante `move`.

El checkpoint del consumidor se consulta antes de escribir, entre informes y
tras cada espera. Durante requests, las esperas de respuesta se dividen en
intervalos de 50 ms conservando el plazo absoluto original. Un timeout parcial
no repite el request ni reinicia el presupuesto. Esto permite cancelación/salud
sin esperar todo el timeout de la consulta. La inicialización del observador
ocurre antes de instalar el consumidor; ahora dispone de un hook de cancelación
separado que conserva la FIFO hasta validar identidad/capacidades (véase
[propietario](native-owner.md)).

Un error o panic recuperable de `consume`/`checkpoint` cierra el transporte sin
reenvío. Un fallo al entregar la FIFO conserva el sufijo no confirmado como
evidencia, sin reproducirlo automáticamente. La entrega puede haber tenido un
efecto parcial antes de fallar. Si ocurre después de enviar un setter, se conserva
`attempted` y no se presume restauración. Un callback lento no extiende el plazo
de la operación: se comprueba al regresar. Las comprobaciones son cooperativas;
no interrumpen un callback bloqueado, SIGKILL, abort ni E/S kernel que no regresa.
El supervisor y el futuro guardián siguen siendo necesarios.

Doce casos nuevos (tres biblioteca, nueve transporte) verifican FIFO antes de
informes nuevos, 300 informes durante una consulta sin llenar la cola, diagnósticos,
setters escalares/botones, ACK y errores HID++, fallos/panics del consumidor,
cancelación durante esperas, rechazo de lectores competidores y plazos sin extensión.
La suite completa pasa con **347 pruebas**, incluidas **136 Rust**. Las pruebas de
transporte requieren sockets Unix locales bloqueados por el sandbox y pasan con
ese acceso. No se realizó nueva prueba física ni se activó diversion. El guardián,
acciones, propietario duradero y aceptación física de eventos siguen pendientes.


## Propietario pasivo

El observador y la opción `native_events --refresh` usan ahora
[`PassiveEventOwner`](native-owner.md). Mantiene la misma sesión a través de
consultas, entrega de eventos y cierre. La segunda lectura y 5001 ms de escucha
en reposo están aceptados en Bolt, sin notificaciones ni escrituras. Los cambios
de identidad/configuración y la recuperación del estado lógico solo tienen pruebas
sintéticas. El propietario es finito y cooperativo, no el guardián ni el servicio
completo de acciones. N4 sigue parcial.


La [restauración nativa de reporting](native-recovery.md) ya dispone de un motor
con registro validado y pruebas sintéticas. Es un motor separado de este
propietario; la integración de ambos pertenece al guardián descrito a continuación.


El [guardián nativo independiente](native-guardian.md) ya conecta propietario pasivo
y restauración, conservando descriptor y lease en el mismo proceso durante todo el
ciclo. Su IPC solo ofrece estado/refresh/cierre. La recuperación tras muerte del
cliente está probada con procesos y mouse sintético; falta aceptación física y
no se habilita diversion ni se producen acciones. Esta integración sustituye el
handoff/herencia HID previsto por propiedad continua en el guardián.
