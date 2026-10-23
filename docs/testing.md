# Pruebas y evidencia

**Actualización del hito de fiabilidad:** consulta [el registro activo](reliability-milestone.md).
Las cifras y resultados de la primera tabla describen la sesión inicial; no sustituyen la
aceptación física completada del nuevo hito.

Sesión inicial: 17 de octubre de 2026 (America/Santiago).

## Comprobaciones ejecutadas

| Prueba | Resultado | Qué demuestra |
|---|---|---|
| `pnpm test` | 13/13 pasan | Validación, contrato, persistencia XDG, permisos del archivo, conservación de JSON corrupto, conflictos, ausencia de simulación, reconexión y desarme del perfil automático con dobles |
| `pnpm build` | Pasa | TypeScript estricto, compilación Vue/Vite |
| `cargo check --offline --manifest-path src-tauri/Cargo.toml` | Pasa | Compilación del host y dependencias GTK/WebKit |
| `pnpm tauri build --debug --no-bundle` | Pasa | Ejecutable Linux con frontend integrado |
| `cargo fmt --check` | Pasa | Formato Rust |
| `bash -n packaging/arch/PKGBUILD` | Pasa | Sintaxis de receta; no verifica instalación |
| `git diff --check`, `git check-ignore` | Pasa | EXE propietario, target y schemas generados excluidos; archivos nuevos revisados separadamente |
| `solaar show` | Hardware real | Bolt C548, MX Master 3S B034, HID++ 4.5, batería, features y estado |
| `python tests/hardware_roundtrip.py` | Pasa en segundo ciclo | Seis escrituras con lecturas posteriores y restauración completa de los valores originales |
| Ventana nativa | Apertura y lectura visual verificadas | Vue → IPC Tauri → Rust → Python → HID++ → Bolt, mostrando 40 % y 1800 DPI, sin dispositivo ficticio |

La primera apertura nativa falló con `Gdk Error 71` de Wayland. Reabrir con
`WEBKIT_DISABLE_DMABUF_RENDERER=1` permitió mostrar la ventana. Se incorporó
el mismo ajuste al inicio exclusivamente en Wayland con módulo NVIDIA cargado,
antes de crear threads/GTK y respetando una variable explícita del usuario.
Fuente: [Tauri Linux Graphics Issues](https://v2.tauri.app/develop/debug/linux-graphics/).
No se modificó configuración personal ni se forzó X11.

Se observó una ventana Wayland (`xwayland: false`) y se inspeccionó una captura
limitada a su región, fuera del repositorio. La prueba de navegación por teclado
no se considera aprobada: el dispatcher tradicional para enfocar no fue aceptado
por esta versión de Hyprland. No se enviaron acciones de escritura por esa vía.
Los cambios físicos verificados se realizaron mediante el backend de la aplicación.
La aceptación posterior de controles y perfiles GUI se recoge al final de este documento.

## Prueba física y recuperación

La prueba optativa requiere cerrar Solaar/logid y Linux ScrollMaster, dejar un único mouse activo
y ejecutarse como usuario con ACL hidraw. No pertenece a `unittest discover`.
El baseline inicial fue DPI 1800, SmartShift 10, modo dentado 2,
inversión vertical false, alta resolución false e inversión horizontal true.

Primer ciclo: las seis escrituras se confirmaron, pero el `finally` rechazó un
`NamedInt` interno de Solaar antes de restaurar. La recuperación también identificó
que SmartShift devuelve el marcador 1 mientras el modo está en giro libre.
Se restauraron explícitamente los seis valores y se verificaron; se corrigió el
script para normalizar a JSON y el adaptador para aplicar modo antes del umbral.
El segundo ciclo finalizó con `PASS original values restored` para todos los ajustes.
No se oculta este fallo inicial ni se confunde con pérdida permanente del dispositivo.

El test cambia temporalmente la sensibilidad y el scroll. No usarlo mientras se
trabaja con el mouse en otra aplicación. No valida la sensación táctil, la precisión
del sensor ni la respuesta de todas las aplicaciones al scroll de alta resolución.

## Pendientes físicos y funcionales

- Bluetooth, múltiples receptores y mismo Unit ID entre transportes.
- Desconectar/reconectar receptor y apagar/encender mouse durante una escritura.
- Autostart XDG voluntario implementado; aceptación al iniciar sesión pendiente.
- Permisos hidraw realmente denegados en sesión limpia; mensajes actuales cubiertos con dobles.
- Fallos reales de disco: cubiertos con dobles, no provocados en el equipo.
- Accesibilidad, navegación de teclado y temas a diferentes escalas de pantalla.
- Paquete Arch instalado en chroot y ruta de recursos en release.

Los dobles existen únicamente en `tests/`. El frontend web sin Tauri avisa que
no puede acceder al dispositivo y no carga fixtures. Las pruebas normales usan
solo la biblioteca estándar Python y no necesitan hardware ni privilegios.

## Suspensión y reanudación

El ensayo con Linux ScrollMaster abierta reanudó correctamente: retorno registrado a 22:27:04
local y lectura real a 22:27:42 con seis ajustes correctos. La prueba funcional
queda aprobada. El diagnóstico general de fallos intermitentes de suspensión de
Linux queda fuera de Linux ScrollMaster por delimitación explícita del usuario. El registro
histórico se conserva en [suspend-incident.md](suspend-incident.md), sin tareas
pendientes ni bloqueo de aceptación.

## Cierre de aceptación funcional

El ciclo GUI → ajustes → guardado → restauración → reconexión automática está
verificado por Bolt con participación del usuario. Los grupos automatizados actuales
suman 40 pruebas (26 Python, 11 estado Vue, 3 Rust). SmartShift, resolución y modo de
rueda se contrastaron con lecturas reales; las inversiones se probaron/revirtieron
por el usuario y se comprobó el estado final. El perfil final conserva alta resolución
por decisión del usuario y su reaplicación quedó habilitada. Detalle consolidado en
[reliability-milestone.md](reliability-milestone.md).

Las dos regresiones más recientes verifican que respuestas IPC antiguas no
anuncian restauraciones tras una desconexión posterior ni sustituyen mensajes
de recuperación más recientes. Ambas fallaron antes de la corrección y pasan
después. No usan hardware ni alteran ajustes del usuario.

## Incremento posterior: botones nativos

La suite actual suma **60 pruebas: 44 Python, 13 estado Vue y 3 Rust**. Las cifras
anteriores describen el cierre del hito de fiabilidad, que permanece aceptado.
Se añadieron pruebas de asignaciones nativas, schema 1/2 y una regresión de versión
Solaar tras crear commits Git. El catálogo se leyó en hardware real; escritura
y aceptación física permanecen pendientes. Véase [native-buttons.md](native-buttons.md).

## Modificadores mantenidos: cierre del incremento anterior

**81 pruebas automatizadas: 64 Python, 14 estado Vue, 3 Rust.** Se verificó en
hardware la restauración tras cierre y SIGKILL del worker, y el usuario confirmó
Ctrl/Alt y liberación real desde GUI. La lectura independiente final confirmó
funciones nativas restauradas; los perfiles permanecieron intactos. Falta la
aceptación física completa del perfil schema 3 y desconexión mientras se mantiene
una tecla. Scripts optativos y evidencia: [held-modifiers.md](held-modifiers.md).

## Acciones de edición: suite vigente

**99 pruebas automatizadas: 81 Python, 15 estado Vue, 3 Rust.** Copiar/Pegar
verificados por el usuario desde GUI con botones reales. Cortar/Deshacer también
fueron confirmados; los botones se devolvieron a sus funciones originales y se
contrastaron por lectura independiente a 03:23:15 UTC. El perfil
schema 4 y sus reconexiones físicas siguen pendientes. Registro detallado en
[button-shortcuts.md](button-shortcuts.md).

## Gestor de perfiles

116 pruebas: 93 Python, 17 estado Vue y 6 Rust. Incluyen gestión offline, preview
sin escrituras, duplicados, referencias activas, límites y persistencia de archivos
exportados. Diálogos GTK compilados; aceptación gráfica pendiente. Véase
[profile-management.md](profile-management.md).

## Incremento de gestos direccionales

132 pruebas aprobadas: 109 Python, 17 estado Vue, 6 Rust. Frontend y host compilan.
Las 16 regresiones nuevas usan dobles, no hardware; detalles y aceptación pendiente
en [directional-gestures.md](directional-gestures.md).

## Incremento de perfiles por aplicación

145 pruebas aprobadas (122 Python, 17 Vue, 6 Rust). Lectura del foco y cinco
aplicaciones mediante IPC real de Hyprland confirmada; no se mutó hardware ni
configuración. Cambio de ajustes al alternar aplicaciones pendiente de aceptación
GUI. Ver [application-profiles.md](application-profiles.md).

Prueba física posterior: `python tests/hardware_modifiers_roundtrip.py --gestures`
aprobó activación raw XY, cierre normal y recuperación tras SIGKILL, con perfiles
intactos. No prueba movimientos físicos ni acciones disparadas por gestos.

Instancia única y bandeja: seis regresiones Rust nuevas aprobadas fuera del sandbox
por su restricción AF_UNIX. Total 151: 122 Python, 17 Vue, 12 Rust. Bandeja pendiente
de aceptación gráfica. Usuario confirmó Copiar/Pegar por gestos izquierda/derecha.

Multimedia en gestos: cinco regresiones nuevas; suite actual 156 (127 Python,
17 Vue, 12 Rust). Salida/combinaciones, guardián y perfiles verificados con dobles;
aceptación física multimedia pendiente. Ver [media-gestures.md](media-gestures.md).

Precisión: cinco regresiones nuevas, total actual 161 (132 Python, 17 Vue, 12 Rust).
Lectura física de 1B04 = versión 5 confirma aplicabilidad del filtro inicial.
Multimedia fue confirmada por usuario; clasificación corregida pendiente de prueba
física. Build release offline completó; kit Arch repetido con SHA idéntico y
`makepkg --verifysource` aprobado. No hubo instalación ni prueba en chroot.

Precisión corregida aceptada por usuario («ahora funciona mucho mejor»). Captura y
restitución real volvieron a pasar tras el cambio. La repetición Rust detectó un
fallo intermitente de reinicio, corregido con unlock explícito/reintento del lease.
Nueva regresión de salida durante activación; total aprobado:
162 pruebas (132 Python, 17 Vue, 13 Rust).


## Distancia estable por DPI

178 pruebas aprobadas: 143 Python, 17 estado Vue, 5 conversión del editor, 13 Rust.
Frontend y ejecutable debug compilados; cargo check/test offline aprobados. La
prueba real `hardware_modifiers_roundtrip.py --physical-distance` confirmó cambio
DPI y captura/restauración tras cierre normal y crash; finally restauró DPI y
perfiles intactos. No emite teclas ni comprueba recorrido físico de la mano.
Comparación GUI entre DPI y perfil schema 8 real pendientes. Evidencia y protocolo
en [gesture-distance.md](gesture-distance.md).

## Atajos personalizados — suite vigente

**199 pruebas aprobadas**: 156 Python, 17 de estado Vue, 5 de distancia, 4 de
formato de atajos, 4 de componentes Vue y 13 Rust. Las 21 regresiones nuevas
incluyen validación canónica, schema 9, cancelación, persistencia y recuperación.
Tres casos Python compilan el C real con transporte simulado y comprueban
límites, pulsación/liberación, máscara anterior y cierre tras fallo. Los SFC reales
se compilan para un renderer Vue en memoria, sin navegador: los borradores no
escriben y los sondeos conservan la edición hasta Aplicar.

La suite regular requiere compilador C, pkg-config y Wayland/xkbcommon de desarrollo;
no abre un compositor ni necesita hardware, root o red. `pnpm test`, `pnpm build`,
`cargo check --offline`, `cargo test --offline` y build debug integrado pasan.
No se hicieron pruebas físicas ni se modificaron perfiles reales. La aceptación
GUI/hardware de atajos y perfiles schema 9 sigue pendiente. Detalles en
[custom-shortcuts.md](custom-shortcuts.md).

## Acciones Hyprland — suite vigente

**215 pruebas aprobadas**: 170 Python, 17 de estado Vue, 5 de distancia, 4 de formato
de atajos, 6 de componentes y 13 Rust. Se añaden 14 casos Python y 2 de componentes:
wire Lua fijo, versión/capacidades, timeout sin reintento, respuesta acotada, cambio
de sesión, cancelación, guardián, preflight antes de diversion, schema 10 y perfiles.
Los tests usan sockets simulados; no ejecutan acciones sobre el compositor real.
Las opciones no disponibles se deshabilitan y cargar una asignación no la ejecuta.

Frontend, `cargo check/test --offline`, formato Rust y ejecutable debug integrado
aprobados. No se cambiaron espacios/ventanas, mouse, perfiles reales ni configuración.
La aceptación GUI/hardware sigue pendiente; detalles en [desktop-actions.md](desktop-actions.md).

## Inicio automático XDG — suite vigente

**233 pruebas aprobadas**: 170 Python, 17 del estado Vue, 5 de distancia, 4 de formato
de atajos, 8 de componentes, 7 de estado de preferencias y 22 Rust. Nueve casos Rust
nuevos comprueban archivos temporales propios, ausencia de escrituras por defecto,
conservación de entradas ajenas/editadas/enlaces, permisos, fallos y cambio de ruta.
El parser GLib/GIO valida las rutas especiales sin ejecutarlas; `%` se rechaza.
Siete casos TypeScript comprueban carga sin activación, aislamiento de preferencias,
concurrencia, lectura tras error y bloqueo hasta recuperar una lectura. Dos casos
de componentes verifican intención explícita y estado confirmado de la casilla.

`pnpm test`, `pnpm build`, `cargo check/test --offline`, formato y ejecutable debug
integrado aprobados. Las pruebas Rust de sockets de instancia se ejecutan fuera del
sandbox del agente. No se escribió ninguna entrada autostart del usuario ni se abrió
Linux ScrollMaster para estas pruebas. La aceptación real está pendiente: [autostart.md](autostart.md).


## N1: codec y transporte Rust experimental (20 de octubre de 2026)

Suite actual: **256 pruebas aprobadas**: 170 Python, 17 de estado Vue, 5 de
distancia, 4 de formato de atajos, 8 de componentes, 7 de preferencias y 45 Rust.
Las 22 nuevas pruebas Rust verifican framing estricto, correlación por dispositivo,
feature/función/software, errores 0x8f/0xff, informes intercalados, cola llena,
timeouts, fallos de E/S y cierre del propietario. Cinco casos prueban `poll` y E/S
de archivos con sockets Unix temporales, incluida contrapresión y un intercambio
con emisor simulado. Fixtures sintéticos independientes del codec, con procedencia
documentada; no se importa Solaar ni se abre hardware.

`pnpm test`, `pnpm build`, `cargo check --offline --manifest-path src-tauri/Cargo.toml`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml`, formato y diff aprobados.
La primera ejecución acotada Rust pasó 18 casos y falló 4 por `EPERM` al configurar
sockets dentro del sandbox. La suite completa posterior fuera del sandbox pasó
los 45 Rust. No fue un fallo de protocolo ni una prueba física del mouse.

La biblioteca no está conectada a la GUI; siguen pendientes N2–N6 y la aceptación
del backend nuevo. Los resultados no certifican latencia del driver, Bluetooth,
escrituras ni recuperación física. Inventario y límites: [transporte nativo](native-transport.md).

## N2: descubrimiento y diagnóstico de solo lectura

Suite actual: **280 pruebas aprobadas**: 170 Python, 17 de estado Vue, 5 de
distancia, 4 de formato de atajos, 8 de componentes, 7 de preferencias y 69 Rust.
Las 24 nuevas comprueban descriptor/filtros, selección/metadata/hotplug, lease y
procesos, supervisión, identidad y getters. Un emulador sintético revisa los
reports enviados para detectar setters. No se abre hardware en la suite regular.

`pnpm test/build`, `cargo check --offline --all-targets`, `cargo test --offline`,
formato y diff aprobados. La suite Rust completa se ejecutó fuera del sandbox
para permitir opciones de sockets Unix de N1 y las pruebas de instancia existentes.

`--list` reconoció el receptor C548/interfaz HID++ real mediante sysfs. `--read`
falló primero por filesystem de sesión de solo lectura dentro del sandbox;
el intento autorizado fuera se detuvo por Solaar activo antes de abrir hardware.
Aceptación de las lecturas físicas pendiente. Detalles: [N2](native-diagnostic.md).

## Aceptación N2 y seis setters N3

Suite actual: **295 pruebas aprobadas**: 170 Python, 17 de estado Vue, 5 de
distancia, 4 de formato de atajos, 8 de componentes, 7 de preferencias y 84 Rust.
Cinco regresiones cierran N2 (metadatos Bolt y modelo BLE sin eQuad); diez casos
N3 cubren validación completa previa, identidad, orden modo/umbral, bits de
reporting, confirmación por getters, errores parciales y sobres diferenciados.
`pnpm test/build`, `cargo check --offline --all-targets`, suite Rust completa,
formato y diff aprobados. Ejemplo `native_settings` compilado.

Con Solaar cerrado por el usuario, metadatos del receptor localizaron el slot 2
en 16 ms. Dos diagnósticos reales aprobaron en 1348/865 ms, con identidad y seis
valores iguales, contraste independiente de raw y perfiles intactos. N2 queda
aceptado por Bolt. Después, la prueba DPI expresamente autorizada confirmó
1800→1850→1800, finally y seis valores restaurados. Lectura independiente final
aprobada en 1253 ms, perfiles intactos y journal eliminado. Los demás setters,
remapeo y fallos físicos de escritura siguen pendientes de aceptación nativa.
Detalles: [lecturas](native-diagnostic.md) y [setters](native-settings.md).

## Remapeo Rust N3

Suite actual: **312 pruebas aprobadas**: las 211 Python/TypeScript anteriores y
101 Rust (53 biblioteca, 23 host, 25 integración). Diecisiete casos nuevos cubren
el catálogo y remapeo secundario, protecciones, readback y errores parciales.
`pnpm test/build`, check de todos los targets y suite Rust completa pasan.

Prueba física autorizada Atrás→Adelante→Atrás aprobada. Lectura independiente final:
tres mappings originales, reporting cero, seis valores restaurados, perfiles con
hash idéntico y journal eliminado. No se dispararon pulsaciones. Evidencia y
limitaciones: [remapeo nativo](native-button-remapping.md). La aceptación ampliada
N3 y recuperación ante crash/desconexión siguen pendientes.

## Base de recepción pasiva N4

Suite actual: **328 pruebas aprobadas**: 211 Python/TypeScript y 117 Rust
(68 biblioteca, 23 host, 26 integración). Quince casos de eventos más una regresión
de transporte verifican filtros, flancos, datos inválidos, deltas con signo,
cancelación, límites, fallos parciales y getters sin setters. Build y check de
todos los targets pasan. Tres pruebas del host requerían sockets Unix bloqueados
por el sandbox; la suite completa pasó fuera de esa restricción.

La herramienta de solo lectura completó 1000 ms en Bolt sin recibir informes.
Esto verifica escucha en reposo; los eventos siguen siendo fixtures sintéticos.
No se habilitó diversion y el hash de perfiles no cambió. N4 requiere guardián y
conservación de notificaciones durante consultas antes de captura activa.
[Evidencia y límites](native-events.md).


## Conservación de notificaciones durante consultas N4

Suite actual: **335 pruebas aprobadas**, con 124 Rust (75 biblioteca, 23 host,
26 integración) y 211 Python/TypeScript. Siete regresiones nuevas verifican FIFO
entre getters, ACK y confirmación, backlog inicial y saturación antes/después de
un setter. El desbordamiento invalida el snapshot y cierra el transporte; después
de un ACK el cambio puede haber ocurrido, sin reenvío ni rollback supuesto.
Check de todos los targets, build y suites pasan; los sockets Unix de las pruebas
del host requieren acceso fuera del sandbox. No hay prueba física nueva.
[Evidencia y límites](native-events.md).


## Consumidor síncrono N4

Suite actual: **347 pruebas aprobadas**, 136 Rust (78 biblioteca, 23 host,
35 integración) y 211 Python/TypeScript. Doce casos nuevos prueban entrega durante
consultas/setters y reposo, FIFO inicial, 300 informes sin saturar la cola, errores,
panics, cancelación y plazos. ACK y errores correlacionados no se entregan como
eventos. Build, check de todos los targets y suite completa pasan. Las pruebas
con sockets Unix locales pasan al permitir ese acceso fuera del sandbox.
Sin nueva prueba física; no se activa diversion. [Detalles](native-events.md).


## Propietario pasivo N4

Suite actual: **357 pruebas aprobadas**, 146 Rust (88 biblioteca, 23 host,
35 integración) y 211 Python/TypeScript. Diez casos nuevos cubren refresh con
eventos, cambio de identidad/configuración, cancelación, fallos, timeouts, snapshot
validado y cierre/destrucción idempotentes. Build, check de todos los targets,
suite completa y formato pasan; las pruebas de sockets usan acceso local permitido.
La CLI `--refresh` confirmó una segunda consulta y 5001 ms de escucha en Bolt,
sin informes ni cambios de perfiles. Esto acredita el ciclo pasivo en reposo;
no los eventos activos ni recuperación. [Evidencia y límites](native-owner.md).


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
