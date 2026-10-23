# N4: restauración nativa de reporting

Incremento del 20 de octubre de 2026. `native/recovery.rs` añade
`ReportingRecovery`: registro opaco de controles secundarios y restauración de
sus flags temporales mediante HID++ propio. El [guardián nativo](native-guardian.md)
ya utiliza esta pieza para restaurar al terminar. El motor por sí solo no incorpora
proceso, teclado ni activación de diversion. No está conectado a la GUI.

## Registro y ejecución

`prepare` toma prestada una sesión y consulta identidad, modelo MX Master 3S,
feature 1B04, catálogo y reporting sin caché. Admite una lista de uno a tres CIDs
únicos entre 83/86/195. Exige controles físicos de mouse, reprogramables y
desviables, con reporting cero y mapping válido. El CID 195 también debe anunciar
RAW_XY. Un control interceptado previamente no puede registrarse. Preparar solo
realiza getters; el registro no activa nada ni sustituye el ACK de un guardián.

El registro conserva slot, Unit ID y descripción de feature/capacidades/destinos
de los controles secundarios anunciados, además del tamaño del catálogo. Sus
campos son privados: no hay constructor desde JSON ni plan inyectable por IPC.
El registro es de un solo uso, sin clonación ni repetición automática.

`restore` consume registro y transporte exclusivo. Antes de cualquier escritura,
vuelve a consultar identidad y descripción del dispositivo. Cambios de índice,
revisión, flags de feature, catálogo o capacidades/destinos secundarios bloquean
la recuperación. Los ajustes escalares y el mapping actual pueden variar: no se
restaura un mapping histórico ni se escriben DPI, ruedas o SmartShift.

| Control registrado | Flags propios que se limpian | Parámetros del setter |
|---|---|---|
| 83 / 86 | Diversion temporal, `0x01` | `[CID_hi, CID_lo, 0x02, 0, 0, 0]` |
| 195 | Diversion temporal y RAW_XY, `0x11` | `[0, 195, 0x22, 0, 0, 0]` |

Solo se envían bits de cambio válido, con valores de activación cero y remap
cero. Se conservan mapping actual, flags persistentes, RAW_XY de laterales y
flags altos/desconocidos ajenos al registro. Los controles no registrados no se
escriben. Si los flags propios ya están a cero, solo se comprueba por lectura.
ACK y lectura posterior deben confirmar los flags esperados y el mismo mapping.
Una última ronda de getters comprueba todos los controles registrados.

## Plazos, informes y errores

Registro y recuperación aceptan presupuestos mayores que cero y de hasta 20 s;
las listas y plazos inválidos fallan antes de E/S. Cada request conserva el límite
de dos segundos y el plazo absoluto global, sin reenvíos. Son límites cooperativos;
no hay supervisor externo que interrumpa syscalls retrasadas.

La sesión de recuperación instala un consumidor fijo que cuenta y descarta
informes no correlacionados, incluidos eventos de botones y respuestas ajenas.
No interpreta ni ejecuta esos eventos. Se necesita para no saturar la FIFO normal
mientras los botones siguen desviados. Tiene un máximo total de 4096 informes;
un informe adicional cierra el transporte y devuelve error. Este descarte solo
pertenece a la recuperación terminal: la captura normal conserva sus garantías.

La salida distingue `completed`, `registered`, `confirmed`, `written`,
`attempted`, `pending`, `discarded_reports` y `error`. `written` enumera setters
que recibieron ACK y lectura posterior correctos. `attempted` identifica un
setter cuyo resultado puede ser incierto; no equivale a rollback. Si falla el
lote, `confirmed` conserva comprobaciones anteriores; al comenzar la ronda final
se vacía y se reconstruye únicamente con sus lecturas correctas. `pending` es
el conjunto registrado sin confirmación. Todo error detiene el lote; no se
continúa escribiendo con estado incierto ni se reabre el dispositivo.

`completed:true` acredita solo la limpieza del alcance registrado con confirmación;
no significa que todos los flags del mouse estén a cero. La API suelta el descriptor
antes de devolver el informe, tanto en éxito como en error. No libera teclas
Wayland: todavía no produce ninguna.

## Pruebas y procedencia

Quince regresiones sintéticas verifican listas/plazos inválidos, capacidades y
reporting previos, limpieza de los tres controles, conservación de mapping/flags
ajenos, no-op, identidad/feature/catálogo distintos, ACK incorrecto, setter sin
efecto, cambios concurrentes, timeout tras escritura, resultados parciales,
rearmado durante confirmación final, respuesta tardía y 300/4097 informes
intercalados antes y después de un setter. Comprueban los bytes de cada escritura
y la destrucción del transporte. No abren hardware ni importan Solaar/Python.

El formato de bits válidos y remap cero se contrastó con la
[especificación Logitech 1B04 v2](https://lekensteyn.nl/files/logitech/x1b04_specialkeysmsebuttons.html)
y análisis estático selectivo de `ReprogrammableKeyV4._setCidReporting` y
`MappingFlag` en el archivo instalado `logitech_receiver/hidpp20.py`. SHA-256:
`b6dfbd88bba8dcbf6b3eb2de5ea1345c2b02f56a67e23799a1b2a5c7f635aaa7`.
Implementación Rust propia; los fixtures son sintéticos, no capturas del mouse.

Suite vigente: **412 pruebas aprobadas**, incluidas 194 Rust
(136 biblioteca, 23 host, 35 integración) y 218 Python/TypeScript.
Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml`, formato y diff.
Las pruebas del host y del guardián requieren sockets Unix locales fuera del sandbox.

## Lo que falta antes de diversion

El llamador debe conservar el lease y garantizar que el lector activo ha cesado
antes de entregar el transporte a recuperación. Esta API no detecta por sí misma
otro lector y no hace seguro duplicar un descriptor. Un EOF de IPC sin comprobar
el fin del propietario no basta para el handoff. Tampoco protege frente a procesos
no cooperativos que cambien el mismo flag; el lease no bloquea controladores ajenos.

El guardián ya conserva el lease y la propiedad del descriptor durante todo el
ciclo, sin transferencia entre procesos. Siguiente: teclado Wayland y acciones,
liberación de teclas, gestos y DPI, y aceptación física específica. El motor actual se detiene ante
el primer error y no promete recuperar un mouse desconectado, un proceso muerto
o una caída del sistema. N4 sigue parcial; la aplicación usa Solaar/Python.


El [guardián nativo independiente](native-guardian.md) ya conecta propietario pasivo
y restauración, conservando descriptor y lease en el mismo proceso durante todo el
ciclo. Su IPC solo ofrece estado/refresh/cierre. La recuperación tras muerte del
cliente está probada con procesos y mouse sintético; falta aceptación física y
no se habilita diversion ni se producen acciones. Esta integración sustituye el
handoff/herencia HID previsto por propiedad continua en el guardián.
