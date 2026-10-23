# N2: descubrimiento y diagnóstico nativo de solo lectura

Incremento del 20 de octubre de 2026. **N2 aceptado en el MX Master 3S por Bolt**:
identidad, batería, capacidades y seis valores reales, repetidos y contrastados
con sus bytes crudos. El primer intento histórico se detuvo por Solaar activo;
el usuario lo cerró y autorizó continuar. No se cambiaron ajustes ni perfiles
para aceptar N2. Las escrituras N3 tienen evidencia separada.

La GUI sigue usando el backend Solaar. Esta herramienta Rust es optativa y está
separada de `snapshot`: no carga perfiles, no reconcilia, no asigna botones y no
habilita diversion. N1 y N2 no declaran la independencia completa del producto.

## Uso

```sh
# Solo sysfs: no abre /dev/hidraw*, no crea el bloqueo y no consulta el firmware.
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_diagnostic -- --list

# Solo tras cerrar Solaar, logid y Linux ScrollMaster por su mecanismo habitual.
# Leer emparejamientos del receptor; no busca ni empareja dispositivos.
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_diagnostic -- --paired hidrawN

# Sustituir hidrawN por una interfaz que aparezca en --list y elegir un slot explícito.
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_diagnostic -- --read hidrawN --slot 2
```

Sin argumentos se ejecuta únicamente la enumeración pasiva. No se admiten rutas
arbitrarias, selección ambigua ni exploración automática de los seis slots.
Un slot apagado/incorrecto puede fallar: no se busca otro dispositivo ni se reenvía
la consulta después de timeout. No ejecutar como root ni cambiar permisos globales.
El descriptor necesita acceso de lectura/escritura para enviar **consultas** HID++;
eso no implica que la herramienta envíe setters de configuración.

El ejemplo se compila con las dependencias GTK/Wayland del crate actual. Su
ejecución no inicia GUI, worker Python, Solaar ni comandos del compositor. No se
incluye automáticamente en los paquetes ni se convierte en el backend por defecto.

## Separación de responsabilidades

| Archivo | Responsabilidad |
|---|---|
| `src-tauri/src/native/descriptor.rs` | Parser acotado de items HID; reports 0x10/0x11 bidireccionales de 7/20 bytes, página vendor ff00, colecciones y pila balanceadas |
| `discovery.rs` | Enumerar sysfs, filtrar bus USB/VID 046D/PID C548 e interfaces convencionales; comprobar nuevamente metadata, número de dispositivo y descriptor abierto con ioctls de lectura |
| `access.rs` | Rechazar root/controladores conocidos; tomar el mismo `linux-scrollmaster-hardware.lock` que el servicio existente, conservando su contenido |
| `pairing.rs` | Lectura acotada del registro de emparejamientos Bolt para localizar slots sin sondear dispositivos ni cambiar vínculos |
| `diagnostic.rs` | Catálogo privado de getters y decodificación de identidad, capacidades y seis valores; errores por lectura, sin valores inventados |
| `runner.rs` | Proceso hijo supervisado, salida máxima 128 KiB, validación del JSON y terminación/recogida ante fallo o timeout |
| `src-tauri/examples/native_diagnostic.rs` | Selección explícita y CLI optativa; nunca se invoca desde Tauri |

Los campos de `Candidate` son privados. La apertura vuelve a validar sysfs, exige
dispositivo de caracteres y major/minor esperados, usa `O_NOFOLLOW` y compara
VID/PID/bus y descriptor con `HIDIOCGRAWINFO`/`HIDIOCGRDESC`. Esto evita aceptar
ciegamente un número hidraw reciclado tras hotplug. No prueba por sí solo la
identidad del mouse emparejado: esa se exige después mediante HID++.

La exclusión usa `XDG_RUNTIME_DIR/linux-scrollmaster-hardware.lock`; exige directorio privado,
archivo regular del usuario sin escritura de terceros, sin enlace simbólico ni
hardlinks adicionales. No trunca el archivo ni modifica perfiles/configuración.
Se comprueban procesos antes de lanzar el hijo y después de tomar el bloqueo.
También se detecta el worker Python de la app si quedó activo tras cerrar GUI.

Esta cooperación no reserva hidraw frente a procesos desconocidos o no
cooperativos. La comprobación por `/proc` tiene la visibilidad de su entorno y
una carrera si un controlador arranca después. El ensayo fuera del sandbox
detectó Solaar que no aparecía en la vista restringida inicial; por eso no se usa
la ausencia dentro del sandbox como evidencia de exclusividad en el host.

## Catálogo de consultas y formato

Se envían reports cortos para root y largos para getters de features, después
de validar ambos en el descriptor. Los registros de emparejamiento usan una
consulta corta fija 83/b5, selectores 51–56 y respuestas correlacionadas también
por el eco del selector; no se ofrece una API genérica de registros HID++ 1.0.
El ping comprueba versión y eco de un token fresco. Las features se resuelven
dinámicamente por root; no se fijan sus índices. El catálogo cubre un subconjunto
conocido, declarado mediante `feature_scope`, no una enumeración exhaustiva.

| Feature | Funciones consultadas | Uso |
|---|---|---|
| Root 0000 | 0, 1 | Resolver IDs y ping/versión |
| Device Information 0003 | 0 | Unit ID, IDs de modelo y transportes |
| Device Name 0005 | 0, 1 | Longitud y fragmentos del nombre |
| Battery 1004 / 1000 | 1 / 0 | Porcentaje cuando existe y códigos crudos de nivel/estado |
| Adjustable DPI 2201 | 0, 1, 2 | Sensor, opciones anunciadas y DPI actuales/default |
| SmartShift 2110 / 2111 | 0 / 1 | Modo y umbral; la opción mejorada tiene prioridad si se anuncia |
| HiRes Wheel 2121 | 0, 1 | Capacidades, inversión y resolución vertical |
| Thumb Wheel 2150 | 1 | Inversión y reporting actuales |
| Reprogrammable Controls 1B04 | Ninguna | Solo presencia/versión por root; no captura ni remapeo |

La identidad requiere Unit ID distinto de cero/ff, modelo Bluetooth LE B034 y nombre exacto
MX Master 3S. No se inventa una identidad a partir del slot. El nombre tiene
longitud acotada y UTF-8 validado. Los layouts de revisión superior a 4, features
ocultas/obsoletas y listas DPI no terminadas en la primera página quedan excluidos
con un error explícito; ampliar cobertura requiere evidencia adicional.

Las opciones DPI se decodifican del anuncio real, con rango/paso y límites de
tamaño. Se valida eco del sensor y pertenencia del valor actual a las opciones;
solo se usa el default anunciado si el valor actual es cero. SmartShift en giro
libre conserva el marcador 1 del contrato y añade `threshold_hidden_in_freespin`;
no lo interpreta como una elección real de umbral mínimo. La batería desconocida
conserva porcentaje nulo y códigos crudos; no se convierte un nivel aproximado
en un porcentaje exacto inventado.

Cada lectura disponible conserva sus bytes `raw`. Una capacidad ausente o un
error de decodificación deja `available:false` con explicación. Un fallo del
transporte invalida el diagnóstico completo, incluso si hay lecturas anteriores.
No se conservan valores en caché entre diagnósticos. Desde la continuación de
[N4](native-events.md), los informes intercalados se conservan en la cola acotada
de la sesión. El diagnóstico independiente cierra su sesión al terminar; no es
un consumidor de acciones. El observador N4 usa la misma sesión y procesa la cola
después de inicializar. Las garantías de captura activa siguen pendientes.

El hijo tiene un presupuesto de 20 s y cada intercambio un máximo de 2 s dentro
del tiempo restante. El padre supervisa 25 s y cierra el hijo sin repetir lecturas.
Esto aísla al llamador de la ejecución normal del driver. Una tarea en E/S kernel
no interrumpible puede retrasar incluso `kill/wait`; no se promete un límite duro
del kernel ni recuperación física certificada por los tests de procesos.

## Fuentes, fixtures y pruebas

- [Logitech: formato HID++ y catálogo de features](https://github.com/Logitech/cpg-docs/blob/master/hidpp20/README.rst)
  y [IRoot](https://github.com/Logitech/cpg-docs/blob/master/hidpp20/features/0x0000-IRoot.rst),
  consultados el 20 de octubre de 2026: resolución de índices y estructura de consultas.
- [Kernel: hidraw](https://docs.kernel.org/hid/hidraw.html) y cabecera local
  `/usr/include/linux/hidraw.h`: interfaz de lectura de metadata y descriptor.
- Inspección estática selectiva del código local de Solaar usado por el MVP:
  `hidpp20.py` (`get_ids`, `get_name`, batería/rueda) y `settings_templates.py`
  (getters y validadores de los seis ajustes). Se contrastan layouts, no se copia
  su implementación. No se importa Solaar como dependencia de la herramienta
  ni como oráculo de sus tests. Hashes SHA-256 de los archivos inspeccionados:
  `b6dfbd88bba8dcbf6b3eb2de5ea1345c2b02f56a67e23799a1b2a5c7f635aaa7`
  y `f1e362d5e5a704b5a7edfa5b0ece0b6a07322db5da9b468d0d86a751a1cdf46a`.

24 pruebas nuevas: descriptor, filtros de descubrimiento, metadata incompleta,
hotplug, selección, exclusión y conservación de archivos, detección de procesos,
supervisión/timeout/salida acotada y consultas del diagnóstico. El emulador usa
Unit ID/índices/valores inventados y revisa las peticiones para detectar setters.
El fixture de descriptor se escribió a mano siguiendo el layout de los bytes
leídos pasivamente en sysfs; no es una captura de consultas del firmware.
Las pruebas normales no abren `/dev/hidraw*`, no necesitan mouse ni Solaar, no
modifican la sesión y solo escriben temporales propios.

Suite vigente tras la continuación: **295 pruebas aprobadas**, con 84 Rust
(29 N2/continuación, 22 N1, 23 existentes y 10 N3). Los cinco casos añadidos a N2
cubren lectura de emparejamientos, correlación por selector, slot inválido,
error de slot ausente e identidad BLE sin eQuad.
`pnpm test`, `pnpm build`, `cargo check --offline --all-targets`, suite Rust completa,
formato y diff comprobados. La suite Rust completa se ejecuta fuera del sandbox
por las restricciones de sockets de las pruebas existentes.

## Evidencia real y aceptación Bolt

`--list` encontró una interfaz compatible `/dev/hidraw7`, receptor 046D:C548,
sin abrirla ni consultar firmware. Las otras interfaces del receptor corresponden
a teclado/mouse y quedaron excluidas. No se declaran nuevos transportes soportados.

El primer `--read` dentro del sandbox falló al abrir el bloqueo de sesión por
sistema de archivos de solo lectura. El intento autorizado fuera del sandbox
devolvió «Cierra solaar antes del diagnóstico nativo» y terminó antes de abrir el
dispositivo. Eso verificó el rechazo del conflicto, no las lecturas. El usuario
cerró después Solaar; el guard pasó y permitió la aceptación que sigue.

El slot 1 devolvió error HID++ 1.0 de recurso 09 aun con el mouse movido. Se
cambió root al formato corto documentado; el error persistió. La nueva lectura
de emparejamientos del receptor encontró B034 en el **slot 2**, en 16 ms.
No se confunde ese error de slot con un fallo demostrado del formato largo.

El primer diagnóstico del slot 2 rechazó el modelo: el código esperaba un ID
eQuad, pero el dispositivo anunció transporte **02 (BLE)** y modelos
`B03400000000`. Se corrigió la validación del campo y se añadió una regresión.
El emulador también corrigió el ID de report al responder largo a un ping corto.

Dos diagnósticos completos independientes aprobaron, en **1348 ms y 865 ms**:

| Dato | Lectura | Contraste |
|---|---|---|
| Identidad | <UNIT_ID>, MX Master 3S, B034 | Coincide con identidad histórica; modelo BLE y tabla del receptor |
| Protocolo | 4.5 | Ping con eco validado |
| Batería | 25 % | Payload inicia `19 02 00 00` |
| DPI | 1800 | Payload inicia `00 07 08 03 E8`; opciones reales 200–8000, paso 50 |
| SmartShift / modo | 10 / 2 | Payload inicia `02 0A 0A` |
| Inversión / resolución vertical | false / false | Modo 00; capacidades inician `0F 1C 18 18` |
| Inversión horizontal | true | Payload inicia `00 01` |

Una decodificación independiente de los bytes y del rango DPI coincidió con las
seis salidas. No importó Solaar ni abrió otro controlador. Hash de perfiles antes
y después de la repetición: `2d83fccaec0f7632efec9f8e9a5920b0e33a1df72532926d21447fced47a93bd`.
No se impuso el baseline histórico de 1850 DPI: 1800 era el valor leído actual.
La evidencia certifica estas consultas Bolt en este equipo; no Bluetooth,
reconexión, errores físicos de escritura ni un límite duro de latencia del kernel.

N3 ya comenzó con [seis setters y prueba DPI optativa](native-settings.md).
