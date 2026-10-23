# Hito de fiabilidad del MVP

**Criterio funcional de aceptación alcanzado en el MX Master 3S por Logi Bolt.**
Se verificó con el usuario el ciclo abrir Linux ScrollMaster → cambiar ajustes → guardar perfil →
restaurar → desconectar/reconectar → recuperar automáticamente los ajustes.
La suspensión y reanudación con Linux ScrollMaster abierta se verificaron correctamente.
El diagnóstico general de suspensión de Linux queda fuera del alcance del proyecto.

Sesión: 17 de octubre de 2026, America/Santiago; las lecturas citadas usan UTC del 18.
No se modificaron configuraciones de sistema, compositor, Solaar ni servicios.

## Cambios implementados

- Adaptador de Solaar aislado en `bridge/solaar_adapter.py`, contrato v1 y versión
  admitida/probada 1.1.20. Restaura el hook de persistencia al cerrar, incluso con error.
- Worker Python residente JSON-lines supervisado por Rust: reutiliza transporte y
  descubrimiento, comprueba conectividad y reabre tras desconexión. Los valores del
  hardware siempre se leen sin caché. Timeout/protocolo inválido descartan y recogen
  el hijo antes de reiniciarlo en la siguiente consulta.
- Sondeo nativo cada 5 s después de completar la consulta anterior; continúa con la
  ventana minimizada. Revisiones monotónicas ordenan eventos y respuestas IPC.
- Estado Vue separado y probado: conserva identidad/perfiles al desconectar, evita
  que respuestas antiguas sobrescriban lecturas nuevas e invalida controles tras error.
- Modelo persistido independiente de Solaar; validación anidada, archivo temporal
  0600, fsync, reemplazo atómico y fsync del directorio. Guardar exige seis lecturas
  disponibles y no sobrescribe un perfil con datos parciales.
- Editar o restaurar desarma la reaplicación antes de escribir: un fallo de disco
  bloquea el cambio y un fallo parcial no reactiva un perfil anterior. Es posible
  armar/desarmar un perfil guardado con el mouse apagado.
- Entrada numérica «DPI exactos», Aplicar/Enter y validación contra opciones reales;
  se conserva el deslizador, ilustración, temas e identidad visual.

## Pruebas automatizadas ejecutadas

| Grupo | Resultado | Alcance |
|---|---|---|
| Python, `pnpm test` | 26 pasan | Adaptador, versión, recursos, validación, persistencia, fallos de disco, perfiles, desconexión/reconciliación y errores |
| Estado Vue, `pnpm test` | 11 pasan | Orden de respuestas, selección estable, errores, recuperación y estado de perfiles |
| Rust, `cargo test --offline --manifest-path src-tauri/Cargo.toml` | 3 pasan | Worker reutilizable, protocolo inválido, timeout/finalización del hijo |
| Compilación | Pasa | `pnpm build`, `cargo check --offline`, `pnpm tauri build --debug --no-bundle` |
| Revisiones finales | Pasan | `cargo fmt --check`, sintaxis PKGBUILD, `git diff --check` |

**40 pruebas automatizadas en total.** Los dobles están exclusivamente en tests y
no representan hardware. No hay automatización WebDriver de clics: la aceptación
GUI fue realizada por el usuario y contrastada con el dispositivo.

## Aceptación con hardware real y participación del usuario

| Caso | Resultado y evidencia |
|---|---|
| DPI desde GUI | Usuario confirmó funcionamiento y fluidez; lectura independiente = 1850 DPI |
| Guardado local | «Mi perfil» creado desde GUI; archivo propio leído y valores contrastados |
| Restauración desde GUI | Después de aplicar 1800, Restaurar recuperó 1850; usuario y lectura independiente confirmaron |
| Desconexión real | Usuario apagó el mouse; lectura 01:12:46: ningún dispositivo activo, perfil conservado |
| Perfil armado offline | Usuario marcó casilla mediante teclado; `active` confirmado antes de encender |
| Reconexión automática | Sin Restaurar/Actualizar, GUI mostró 1850 y mensaje de escrituras confirmadas; lectura 01:14:35 coincidió con los seis valores del perfil |
| Apertura tras reinicio | Lectura previa 01:25:03: 1000 DPI, lateral false. Tras abrir Linux ScrollMaster, 01:25:36: 1850 DPI y lateral true, iguales al perfil persistido |
| SmartShift desde GUI | Lecturas 01:30:31 y 01:30:56: umbral 11, después restaurado a 10 |
| Alta resolución desde GUI | Lecturas confirmaron true; usuario decidió guardarla como preferencia final |
| Inversiones vertical/lateral | Usuario probó y devolvió ambas; lecturas finales contrastaron su retorno. No se capturaron de forma independiente los estados intermedios |
| Giro libre desde GUI | Usuario comprobó físicamente; lectura 01:32:45: modo 1, SmartShift 1 como marcador esperado. Restauración final: modo 2 |
| Suspensión con Linux ScrollMaster abierta | Nuevo ensayo autorizado pasó: journal suspend 22:26:06 local, retorno 22:27:04 y servicios completados 22:27:07. Lectura 22:27:42: seis valores correctos |

La recuperación por reconexión se distinguió de retención de firmware: se preparó
un valor distinto del perfil, se armó con el mouse apagado y la GUI recibió una lista
de escrituras confirmadas no vacía. El ensayo correcto de suspensión demuestra
recuperación/continuidad en ese intento; no demuestra que hubiera que reescribir
valores al despertar.

`tests/hardware_observe.py` realiza lecturas independientes y coopera con el flock.
No usa snapshot del servicio, que sí puede reconciliar y escribir perfiles.

## Latencia

El usuario detectó la lentitud del primer diseño y la dificultad de introducir DPI.
Medidas reales de solo lectura:

| Operación | Tiempo |
|---|---|
| Enumeración repetida del diseño inicial | 2,409 s |
| Ajuste individual leído | 0,015–0,022 s |
| Lectura completa inicial del adaptador residente | 2,648 s |
| Lecturas completas posteriores | 0,128 y 0,142 s |

El usuario confirmó «funciona bien» después del worker residente y los DPI numéricos.
Estos tiempos son lecturas medidas; no se presentan como latencia de escritura GUI.

## Estado final confirmado

El usuario pidió conservar alta resolución y guardó el cambio en «Mi perfil».
La lectura 01:34:30 y la respuesta final del servicio confirmaron coincidencia:

| Ajuste | Valor |
|---|---|
| DPI | 1850 |
| SmartShift | 10 |
| Modo | Dentado, 2 |
| Inversión vertical | false |
| Alta resolución | true |
| Inversión lateral | true |

La edición había desarmado la reaplicación. Se volvió a habilitar «Mi perfil» por el
servicio de Linux ScrollMaster, con respuesta `ok=true`, `active` confirmado y los seis valores
releídos. Esta última habilitación se hizo mediante el servicio; la casilla GUI ya
se había verificado con el usuario durante la preparación offline de reconexión.
No se revirtieron sus preferencias a los valores anteriores a la sesión.

## Límites y pendientes

- Permisos denegados y fallos de almacenamiento cubiertos con pruebas automatizadas;
  no se alteraron ACL del equipo ni se provocaron fallos reales de disco.
- Bluetooth, extracción del receptor, múltiples receptores y desconexión exactamente
  durante una escritura no se probaron físicamente.
- El sondeo requiere que el proceso Linux ScrollMaster permanezca abierto; no hay autostart ni
  servicio instalado. La primera detección/reapertura puede tardar varios segundos.
- Solaar distinto de 1.1.20 requiere verificar y actualizar explícitamente el adaptador.
- No se añaden botones, gestos u otras funcionalidades ajenas al hito.

## Consolidación posterior

Se corrigió una carrera de mensajes: una respuesta IPC antigua ya no anuncia
éxito después de una desconexión más reciente ni sustituye una confirmación
posterior de recuperación automática. Dos regresiones del estado Vue reproducen
ambos casos; la suite pasa con 26 pruebas Python, 11 de estado Vue y 3 Rust.
El historial de suspensión se conserva como contexto, sin pendiente ni bloqueo
del proyecto, por delimitación explícita del usuario.

## Desarrollo posterior

Este registro conserva las 40 pruebas del cierre de fiabilidad. La suite vigente
suma 60 al añadir asignaciones nativas de botones y la corrección de identificación
de versión Solaar. Véase [native-buttons.md](native-buttons.md) para separar la
implementación nueva, las lecturas físicas y la aceptación de escritura pendiente.

El incremento siguiente añade Alt/Ctrl mantenidos con guardián. Ese incremento sumó
81 pruebas; el usuario verificó pulsar/soltar y se probó recuperación real
al cerrar o matar el worker. Véase [held-modifiers.md](held-modifiers.md).

El incremento de acciones de edición eleva la suite actual a 99 pruebas y tiene
aceptación física de las cuatro acciones. Véase [button-shortcuts.md](button-shortcuts.md).

Este documento conserva el corte histórico de fiabilidad. La continuación actual
alcanza 233 pruebas (170 Python, 17 de estado Vue, 5 de distancia, 4 de formato de
atajos, 8 de componentes, 7 de estado de preferencias y 22 Rust) y añade gestos,
multimedia, atajos personalizados, inicio automático XDG voluntario,
acciones de espacios/ventanas Hyprland,
gestión de perfiles, reglas Hyprland e instancia/bandeja. Su aceptación específica
está registrada en plan.md y los documentos de cada incremento. No atribuirla
a las 40 pruebas originales de este hito.


## N1: transporte nativo experimental (20 de octubre de 2026)

Suite actual: **256 pruebas aprobadas**: 170 Python, 17 de estado Vue, 5 de
distancia, 4 de formato de atajos, 8 de componentes, 7 de preferencias y 45 Rust.
Las 22 nuevas pruebas del codec/transporte Rust usan fixtures sintéticos y sockets
locales. `pnpm test/build`, `cargo check/test --offline`, formato y diff aprobados.
La suite Rust se ejecutó fuera del sandbox para permitir opciones de sockets Unix.

Este incremento no accedió al mouse ni cambió perfiles. El hito físico anterior
sigue correspondiendo al adaptador Solaar; no acredita el backend nativo nuevo.
N1 permanece separado del worker activo y N2–N6 están pendientes. Evidencia,
procedencia y límites: [transporte nativo](native-transport.md).

## N2: diagnóstico nativo, aceptación de lecturas pendiente

Suite actual: **280 pruebas aprobadas**: 170 Python, 17 de estado Vue, 5 de
distancia, 4 de formato de atajos, 8 de componentes, 7 de preferencias y 69 Rust.
N2 añade 24 pruebas y una herramienta Rust optativa de solo lectura. Pasan tests,
build, check de todos los targets, formato y diff. El receptor/interfaz se enumeró
realmente por sysfs, pero las consultas se detuvieron por Solaar activo. No se
modificaron hardware ni perfiles. El hito de fiabilidad físico anterior pertenece
al backend Solaar; N2 no hereda su aceptación. [Evidencia y límites](native-diagnostic.md).

## N2 aceptado por Bolt y N3 parcial

Suite actual: **295 pruebas aprobadas**, incluidas 84 Rust (cinco regresiones de
cierre N2 y diez casos N3 nuevos). Build, check de todos los targets, suite,
formato y diff aprobados. N2 tiene aceptación propia: Solaar cerrado, slot 2
identificado, identidad BLE B034/Unit ID <UNIT_ID> y seis valores confirmados en
dos lecturas nativas independientes, con raw contrastado y perfiles intactos.
Lecturas actuales: 1800 DPI, SmartShift 10, modo 2, inversión/resolución vertical
false e inversión horizontal true; no se impuso el perfil histórico.

Los seis setters N3 tienen pruebas automáticas y herramienta separada con
confirmación. Prueba física DPI autorizada y aprobada: 1800→1850→1800, con seis
valores restaurados, lectura independiente posterior (1253 ms), perfiles intactos
y journal eliminado. Otros setters, remapeo y aceptación ampliada pendientes.
La GUI sigue usando el backend anterior.
Evidencia separada: [N2](native-diagnostic.md), [N3](native-settings.md).

## Continuación N3: remapeo secundario

Suite actual: **312 pruebas aprobadas**, incluidas 101 Rust; diecisiete casos nuevos
de catálogo, validación y remapeo 1B04 sin diversion. Check de todos los targets,
suite y build pasan. Roundtrip físico autorizado Atrás→Adelante→Atrás y lectura
independiente final aprobados: tres funciones originales, flags cero, seis ajustes
restaurados, perfiles intactos y journal eliminado. Sigue pendiente la aceptación
ampliada N3; no se conecta el backend experimental a la GUI.
[Evidencia y límites](native-button-remapping.md).

## Inicio N4: recepción pasiva

Suite actual: **328 pruebas aprobadas**, incluidas 117 Rust; dieciséis casos
adicionales de eventos y plazo de recepción. Build, check de todos los targets y
suite completa pasan. Una escucha Bolt de solo lectura terminó en 1000 ms, sin
eventos y con perfiles intactos. No acredita captura ni restauración física.
El observador no activa diversion; guardián y acciones todavía pendientes.
[Contrato y límites](native-events.md).


## N4: FIFO entre consultas y confirmaciones

Suite actual: **335 pruebas aprobadas**, incluidas 124 Rust. Se preservan informes
intercalados por el mismo propietario con límite global de 128 y cierre ante
saturación. Siete regresiones nuevas verifican orden, capacidad y fallos parciales
antes/después del ACK. Frontend, build, check y suite completa pasan.
Sin nueva aceptación física ni activación de diversion; guardián y consumidor
continuo pendientes. [Contrato y límites](native-events.md).


## N4: entrega síncrona durante intercambios

Suite actual: **347 pruebas aprobadas**, incluidas 136 Rust. Doce casos nuevos
verifican consumidor único, FIFO/reposo, diagnósticos/setters, errores/panics,
cancelación y plazos sin reenvíos. Build, check y suite completa pasan con acceso
a sockets Unix locales. El observador usa este consumidor con sus límites; aún
faltan guardián, acciones y aceptación física antes de activar diversion.
[Contrato y límites](native-events.md).


## N4: propietario de sesión pasiva

Suite actual: **357 pruebas aprobadas**, incluidas 146 Rust; diez regresiones de
refresh, identidad/configuración, fallos y cierre. Build, check y suite completa
pasan. En Bolt se verificó segunda consulta con consumidor instalado, 5001 ms
sin notificaciones y cierre normal; perfiles intactos. Falta el guardián y la
recuperación antes de habilitar diversion. [Contrato y límites](native-owner.md).


## Botones de perfiles y guardián finito N5

Suite vigente: **668 pruebas aprobadas**: 178 Python, 41 TypeScript y 449 Rust
(371 biblioteca, 20 host, 44 integración y catorce CLI). Catorce casos nuevos
cubren compilación del mapa persistido, separación de firmware/software,
configuración exacta, procesos con mouse sintético y CLI. Se verifica rechazo de
remapeos nativos discrepantes antes de diversion, detección de cambio durante
activación parcial, acciones solo en software y restauración. Pasan pnpm
test/build, cargo check/test offline --all-targets, formato y diff; las suites de
sockets sintéticos requirieron salir de la restricción del sandbox. Los 128
archivos de código propios cumplen el máximo de 1000 líneas; mayor: 971.
No se abrió hardware, compositor ni perfiles reales. La aceptación física previa
no certifica esta ruta; servicio duradero y sustitución de Python/Solaar siguen
pendientes. [Contrato y evidencia](native-profile-actions.md).


## Propietario independiente de perfiles N5

Suite vigente: **693 pruebas aprobadas**: 178 Python, 41 TypeScript y 474 Rust
(392 biblioteca, 20 host, 48 integración y catorce CLI). Se añaden 25 entradas de
suite: 21 biblioteca (incluidos dos harnesses de procesos) y cuatro integraciones.
Cubren proxy/propietario persistente, tickets y plazo absoluto, desarmado previo,
fallo parcial/recibo incorrecto, cierre con lease retenido, SIGKILL del cliente,
framing y shutdown sin éxito falso. El binario de producción pasó handshake,
rechazo de software y cierre con canal privado sin solicitar hardware. Pasan
pnpm test/build, cargo check/test offline --all-targets, formato y diff; sockets
sintéticos fuera del sandbox. Los 133 archivos propios cumplen 1000 líneas como
máximo (mayor: 971). No se abrió hardware, compositor ni perfiles reales. La
ruta nueva no habilita acciones/software ni acredita recuperación física; la GUI
conserva Solaar/Python. [Contrato y límites](native-profile-guardian.md).


## Consumidor de perfiles duradero N5

Suite vigente: **720 pruebas aprobadas**: 178 Python, 41 TypeScript y 501 Rust
(419 biblioteca, 20 host, 48 integración y catorce CLI). Hay 27 casos nuevos: ocho
consumidor, trece backend/protocolo, dos dueño y cuatro procesos/servidor. Cubren
registro acotado por sesión larga, filtros/XY, FIFO inicial, 300 informes durante
consulta, presupuesto por operación, layout/capacidades, EOF, efecto de remapeo
incierto sin reintento, lease/tickets y equidad IPC entre eventos. Pasan pnpm
test/build, cargo check/test offline --all-targets, formato y diff. Los sockets
sintéticos requirieron salir del sandbox tras errores de permisos dentro.
Los 137 archivos de código propios cumplen 1000 líneas como máximo (mayor 971).
No se abrió hardware, compositor ni perfiles reales. No se habilitó diversion,
salida de software o backend nativo en GUI. La evidencia física previa no
certifica este consumidor. [Contrato y límites](native-profile-events.md).


## Captura residente optativa de perfiles N5

Suite vigente: **750 pruebas aprobadas**: 178 Python, 41 TypeScript y 531 Rust
(447 biblioteca, 20 host, 50 integración y catorce CLI). Hay 30 entradas nuevas:
28 de biblioteca (incluidos dos harnesses) y dos integraciones del ejecutable.
Cubren lotes mixtos, modificadores/gestos, FIFO/sincronización, memoria/tickets,
salida/escritorio fallidos, ACK incierto, EOF/cancelación, procesos/SIGKILL del
cliente, lease y errores de cierre persistentes. El shutdown no confirma stopped
si la salida o restauración fallan. El nuevo modo y los guardianes de producción
también se prueban offline sin solicitar hardware.

Pasan pnpm test/build, cargo check/test offline --all-targets, formato y diff.
Pruebas de sockets fuera del sandbox tras errores de permisos. Los archivos de
código propios conservan el máximo de 1000 líneas. No se abrió mouse, compositor
ni perfiles reales. El modo separado --serve-guarded-capture sí implementa acciones
y restauración residentes, con plan fijo hasta cierre. Las transiciones, la
aceptación física y la paridad de la GUI siguen pendientes; esta conserva
Python/Solaar. [Contrato, evidencia y límites](native-profile-capture.md).
