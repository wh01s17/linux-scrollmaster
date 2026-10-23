# N4: propietario persistente de solo lectura

Incremento del 20 de octubre de 2026. `native/owner.rs` reúne sesión, identidad,
snapshot validado, consumidor y cierre en `PassiveEventOwner`. El observador
existente usa este propietario; `native_events --refresh` permite una segunda
consulta completa sin cambiar de descriptor ni consumidor. **N4 sigue parcial**:
no habilita diversion, acciones ni integración en la GUI.

La variante interna `open_with_sink` ya permite [entrega directa tipada](native-action-delivery.md)
al motor. El guardián usa esa ruta con asignaciones vacías en su comprobación de
teclado, o con asignaciones explícitas en [captura optativa](native-capture.md).
`activate_registered` es interno, exige registro de recuperación y confirma cambios
antes de armar la salida. La API pública
descrita abajo sigue siendo pasiva. Con salida interna, el cierre también libera
el teclado y puede esperar su confirmación acotada antes de restaurar firmware.

## Ciclo de vida y contrato

| API | Comportamiento |
|---|---|
| `open` | Valida duración mayor que cero y hasta 30 s, slot e identidad; consulta catálogo/estado cancelable con presupuesto de 20 s; después instala el consumidor y entrega la FIFO inicial |
| `poll` | Recepción pasiva con esperas de hasta 50 ms, limitada por el presupuesto restante |
| `refresh` | Consulta completa con el mismo descriptor, consumidor y Unit ID; timeout mayor que cero y hasta 20 s |
| `last_snapshot` | Evidencia cacheada de la última consulta íntegra validada; sin E/S |
| `status` | Snapshot, intentos/consultas confirmadas y evidencia parcial de eventos/cierre |
| `stop` | Cierre local inmediato e idempotente, sin E/S; conserva evidencia y tiempo terminal |
| Destrucción | Suelta el descriptor y limpia estado lógico, sin consultas ni reconexión |

El transporte y la sesión son privados: no se exponen lectores, setters ni un
dispatcher de comandos. Descubrimiento validado, guard de Solaar/logid/GUI y
`HardwareLease` siguen siendo responsabilidad del llamador durante toda la vida
del propietario. La CLI mantiene ese lease hasta terminar.

El presupuesto de observación empieza tras el setup. Es una API finita y
cooperativa, de hasta 30 s, con límites totales de 128 eventos/4096 informes.
No hay hilo autónomo que cierre al agotarse el tiempo: el checkpoint lo evalúa
durante `poll` o `refresh`, y la destrucción también cierra. No es todavía el
servicio de sesión ni un demonio; tampoco drena eventos para ejecutar acciones.

La nueva consulta valida Unit ID y compara feature 1B04, revisión/índice/flags,
tamaño del catálogo, controles, capacidades, destinos anunciados, mapping efectivo
y reporting con el baseline. Un cambio cierra con `configuration-changed`.
Los seis ajustes escalares pueden actualizarse sin cambiar esa asociación de
botones. Solo una consulta completa compatible incrementa `refreshes` y sustituye
`last_snapshot`; `refresh_attempts` cuenta intentos con argumentos válidos y
propietario abierto.

Durante la consulta los informes se entregan al mismo consumidor y los eventos
llevan `during_query:true`. Son evidencia pasiva: si falla la consulta o cambia
la configuración, no deben tratarse como acciones verificadas. La última lectura
válida se conserva junto con el error y los eventos parciales. No hay reintento,
reapertura ni restauración implícita. Una consulta incompleta al vencer el
presupuesto general termina con `completed:false`; si el checkpoint detecta ese
vencimiento se informa `query-timeout`.

Cualquier fallo de transporte, consumidor, identidad o consulta cierra antes de
devolver el estado terminal. Cancelación, evento inválido y límites conservan su
causa específica. El conjunto lógico de botones mantenidos se limpia una vez;
`cleared_held` y `elapsed_ms` quedan congelados. Esto no libera teclas Wayland ni
restaura firmware: no se modifica ninguno. Una instancia cerrada no vuelve a
consultar ni escuchar.

## Herramienta de comprobación

```sh
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_events -- --observe hidraw7 --slot 2 --device <UNIT_ID> --seconds 5 --refresh
```

`--refresh` realiza una consulta completa después de instalar el consumidor,
antes de continuar escuchando hasta el presupuesto total elegido. La consulta
comparte ese presupuesto; si no termina, la CLI devuelve fallo con evidencia.
No es un muestreo periódico. Sin el flag se conserva el flujo anterior.
Los argumentos son explícitos y no aceptan rutas arbitrarias.

La inicialización comprueba el mismo callback de cancelación antes de cada E/S
y entre informes, con esperas de lectura de hasta 50 ms. Todavía no instala el
consumidor: conserva la FIFO acotada hasta validar identidad/capacidades. Las
esperas silenciosas no reenvían la consulta ni extienden su plazo absoluto.
Cancelación o panic recuperable del callback cierran el descriptor y devuelven
error desde `open`; no se publica un snapshot parcial. Al instalar el consumidor,
este asume el callback y la sesión retira el hook de inicialización. Los callbacks
deben retornar pronto y no realizar E/S HID ni reentrar en la sesión. El intervalo
es cooperativo, sin garantía de tiempo real ante retrasos de syscalls.
Los checkpoints del consumidor funcionan durante las consultas posteriores. No se configura
el escritorio ni se activan otros controladores.

## Pruebas y evidencia

Diez regresiones sintéticas nuevas verifican consultas repetidas con eventos;
cambios de feature/capacidades/catálogo/mapping/reporting; identidad y desconexión;
cancelación antes de E/S; eventos inválidos; timeout de consulta; snapshot no
reemplazado tras fallo; argumentos inválidos; cambios escalares compatibles;
cierre/destrucción e idempotencia con tiempo terminal congelado. Usan el emulador
independiente N3 y un transporte que cuenta destrucciones. No abren hardware.

Cuatro regresiones adicionales cubren cancelación y panic antes de E/S,
interrupción de identidad/catálogo/reporting, cancelación durante espera silenciosa
y continuación de una espera sin cancelación conservando FIFO y una sola consulta.
Son pruebas sintéticas, sin nueva aceptación física.

Suite: **486 pruebas aprobadas**: 218 Python/TypeScript y 268 Rust
(204 biblioteca, 23 host, 35 integración, seis CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml` y formato. Las pruebas
con sockets Unix locales se ejecutan fuera de la restricción del sandbox.

Comprobación física de solo lectura: Bolt `hidraw7`, slot 2, Unit ID <UNIT_ID>,
con guard de procesos y lease aprobados:

| Comprobación | Resultado |
|---|---|
| Segunda lectura | `refresh_attempts:1`, `refreshes:1`, identidad/configuración compatibles |
| Observación | `completed:true`, `stop_reason:"deadline"`, `running:false`, 5001 ms |
| Notificaciones | Cero informes/eventos; no acredita captura activa |
| Botones en segunda lectura | 83→83, 86→86, 195→195, flags cero |
| Ajustes en segunda lectura | DPI 1800, SmartShift 10, modo 2, inversión/resolución vertical false, inversión horizontal true |
| Perfiles antes/después | SHA-256 idéntico: `2d83fccaec0f7632efec9f8e9a5920b0e33a1df72532926d21447fced47a93bd` |

Salida temporal: `/tmp/scrollmaster-native-owner-refresh.json`. No hubo escrituras
de ajustes, botones ni diversion. Se acepta consulta/escucha en reposo y cierre
normal. Eventos intercalados, fallos y cambios de identidad solo tienen evidencia
sintética. Guardián independiente, recuperación ante crash, acciones y aceptación
física de eventos siguen pendientes antes de captura activa. La GUI continúa con
Solaar/Python.


La [restauración nativa de reporting](native-recovery.md) ya dispone de un motor
con registro validado y pruebas sintéticas. Es un motor separado de este
propietario; la integración de ambos pertenece al guardián descrito a continuación.


El [guardián nativo independiente](native-guardian.md) ya conecta propietario pasivo
y restauración, conservando descriptor y lease en el mismo proceso durante todo el
ciclo. Su IPC solo ofrece estado/refresh/cierre. La recuperación tras muerte del
cliente está probada con procesos y mouse sintético; falta aceptación física y
no se habilita diversion ni se producen acciones. Esta integración sustituye el
handoff/herencia HID previsto por propiedad continua en el guardián.
