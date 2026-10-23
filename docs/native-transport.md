# N1: codec y transporte nativo experimental

Incremento del 20 de octubre de 2026. Biblioteca Rust en `src-tauri/src/lib.rs`,
separada del ejecutable y del worker activo. No abre dispositivos al iniciar,
no tiene comando Tauri y no selecciona automáticamente un backend. La aplicación
actual todavía necesita Solaar 1.1.20 y Python. N2–N6 siguen pendientes.

## Inventario del contrato que debe conservar la migración

| Frontera actual | Datos / operaciones | Garantías a conservar |
|---|---|---|
| Vue → `main.rs::request` | Objeto JSON, sin rutas ni ejecutables del cliente | Trabajo bloqueante fuera del runtime async; revisión monotónica dentro del mutex compartido con sondeo |
| Host → `bridge.rs::Worker` | JSON-lines, petición máxima 256 KiB, respuesta máxima 2 MiB | Una solicitud por vez; timeout actual 45 s; matar y recoger hijo ante fallo |
| Servicio → cliente | `{ok:true,data}` o `{ok:false,error}`, más `revision` del host | No anunciar éxito ni conservar controles habilitados sin lectura válida |
| `snapshot` | `devices`, `profiles`, `profile_version`, `active`, `rules`, `desktop`, `warnings`, `reconciliation`, `diagnostic` | Puede reconciliar perfiles armados: **no** reutilizarlo como diagnóstico nativo de solo lectura en N2 |
| Dispositivo | `id`, `name`, `connection`, `protocol`, `battery`, `settings`, `buttons`, `adapter` | Identidad estable; batería ausente no simulada; disponibilidad/error por capacidad; metadatos de backend explícitos |
| `set` / `remap` | Identidad y diccionario `changes` | Capacidades reales antes de escribir; desarmar automatismo en disco antes de mutar; confirmar por lectura sin caché; informar cambios parciales |
| `save` / `restore` / `auto` | Identidad, nombre de perfil o desarme | Guardar solo lecturas completas; restaurar no arma; selección offline permitida para perfiles existentes |
| Operaciones locales | `rename`, `delete`, `export`, `import_preview`, `import`, `rule_set`, `rule_delete`, `applications` | Validación, referencias consistentes e importación sin activar perfiles ni reglas externas |
| Archivos y escritorio | Schemas 1–10; rutas XDG `linux-scrollmaster`; preferencias, autostart, diálogos Tauri | Preservar datos y escrituras atómicas; foco/salida Wayland separados del transporte |
| Eventos internos | Pulsaciones, raw XY, cancelación, guardián | Un lector; restauración del reporting y liberación de teclas antes de habilitar diversion |

Fuentes del inventario: `src-tauri/src/main.rs`, `src-tauri/src/bridge.rs`,
`bridge/service.py`, `bridge/profiles.py`, `bridge/solaar_adapter.py`, arquitectura
y plan. Es una lista de obligaciones de migración, no una declaración de paridad
del módulo N1. N5 resolverá los metadatos `adapter` del backend nuevo sin fingir
una versión Solaar. La biblioteca N1 no implementa operaciones de producto.

## Diseño implementado

- `codec.rs`: reports cortos/largos, tamaño exacto, padding cero, validación de
  parámetros y errores tipados. No interpreta batería, features ni ajustes.
- `session.rs`: una petición en curso mediante acceso mutable exclusivo. Correlación
  por dispositivo, feature, función y software ID, incluso si cambia el tamaño de
  la respuesta. Identificadores rotan entre 8 y 15; esta elección local no reserva
  identificadores frente a otros procesos ni reemplaza el control de conflictos.
- Informes intercalados se conservan completos y en orden en una cola de 128.
  Incluye notificaciones, eventos específicos de aplicación y respuestas ajenas;
  no se supone que todo informe no correlacionado sea un evento de botones.
  `receive` permite lecturas idle con el mismo propietario y vacía primero la cola.
- Cola llena produce error explícito y cierra el transporte; los 128 informes
  anteriores siguen disponibles mediante `pop_pending`. No se silencian pérdidas.
  N4 añade consumo síncrono durante la transacción mediante `ReportConsumer`,
  después de validar identidad/capacidades. Sin consumidor se conserva el modo FIFO
  original. La recuperación antes de capturar eventos reales sigue pendiente;
  esta cola por sí sola no habilita diversion.
- Un único plazo monotónico abarca escritura y espera de respuesta. No se reinicia
  al llegar notificaciones ni al reintentar syscalls interrumpidas/no disponibles.
  Timeout, desconexión, framing inválido o fallo de escritura descartan el
  transporte y dejan la sesión cerrada. No se reenvía la solicitud automáticamente.
  Un error HID++ correlacionado se devuelve sin reintento y permite otra petición;
  un intervalo idle sin informes es normal y no cierra la sesión.
- `hidraw.rs` usa `File` propio, `O_NONBLOCK`, `poll`, `read` y `write` Linux;
  se añade dependencia directa de `libc`, ya disponible en el lockfile. No se
  incorpora una biblioteca que implemente HID++ ni código de Solaar. Una escritura
  parcial falla: nunca se manda el resto como un report separado. Se lee un byte
  adicional al máximo admitido para detectar informes demasiado grandes.

Se eligió acceso hidraw directo para conservar límites de report y propiedad del
descriptor, sin otra capa de protocolo. `Hidraw::from_file` requiere transferir una
descripción de archivo única: el llamador no debe conservar `dup`/`try_clone` ni
otro lector. No hay apertura de rutas, enumeración, permisos ni IPC expuestos.
La futura capa N2 debe comprobar interfaz/modelo y conflictos antes de abrir.

Los plazos se comprueban alrededor de la E/S; una syscall ya dentro del driver
no puede ser interrumpida por esta biblioteca. No se acredita una latencia máxima
del kernel/controlador físico mediante los sockets de prueba. N2 debe medirla y
resolver el aislamiento necesario antes de integrar el transporte en la aplicación.
Cerrar la sesión tampoco certifica que una escritura no haya llegado al firmware.
No hay rollback ni aceptación de recuperación de hardware en este incremento.

Tras un fallo ambiguo no se debe construir otra sesión para repetir una escritura.
N2/N3 deben resolver respuestas pendientes, comprobar conectividad/identidad y
releer el estado. La rotación de IDs no protege por sí sola frente a una respuesta
arbitrariamente tardía de una sesión anterior. Solo se implementan intercambios
con una respuesta; features con secuencias o eventos requieren una capa específica.
Reports 0x12, protocolo de registros HID++ 1.0 y Bluetooth no se declaran soportados.

## Fuentes y procedencia de los fixtures

Consultadas el 20 de octubre de 2026, sin ejecutar ni importar Solaar:

1. [Logitech HID++ 2.0 Draft Specification, 04-06-2012, pp. 1–3](https://lekensteyn.nl/files/logitech/logitech_hidpp_2.0_specification_draft_2012-06-04.pdf):
   estructura de reports 0x10/0x11 de 7/20 bytes, campos de correlación, software
   ID no cero y ejemplo de ping. El documento es un borrador histórico; no prueba
   comportamiento del MX Master 3S actual.
2. [Linux, hid-logitech-hidpp.c, commit 5b10ff013e8a57f8845615ac2cc37edf7f6eef05](https://kernel.googlesource.com/pub/scm/linux/kernel/git/torvalds/linux/+/5b10ff013e8a57f8845615ac2cc37edf7f6eef05/drivers/hid/hid-logitech-hidpp.c):
   contraste del layout de error 0x8f/0xff y eco de la feature/función originales.
   No se copia su implementación ni su política de reintentos.
3. [Kernel Linux: API hidraw](https://docs.kernel.org/hid/hidraw.html):
   reports numerados incluyen el ID en el primer byte de lectura/escritura;
   lectura no bloqueante y diferencias con un flujo de bytes.

`src-tauri/tests/fixtures/hidpp-n1.txt` contiene vectores hexadecimales **sintéticos,
escritos a mano**, no capturas ni lecturas certificadas. Dispositivo 1, software 8,
token a5, índice ficticio 4 y valores de ejemplo son elecciones de la prueba.
`ping_request/reply` siguen el layout del ping del borrador; `feature_request/reply`
usan la consulta root de feature 0x1000 con un índice inventado. `notification`
solo representa un report intercalado con software cero, sin afirmar una semántica
de batería. `error_v1/v2` reproducen el layout con códigos de ejemplo 1/8.
Los fixtures no se generan con el codec probado y los tests no dependen de Solaar.

## Validación y siguiente incremento

22 casos Rust nuevos: codec, campos y longitudes, errores, correlación, orden,
presión de cola, cierre, timeout y respuesta tardía. Cinco usan sockets Unix
temporales para probar la E/S real de archivos y `poll`, incluyendo el intercambio
completo con un emisor simulado y contrapresión de escritura. No abren `/dev/hidraw*`.
Los sockets preservan reports en los casos de datos; el caso EOF usa un stream.

Ejecutar `cargo test --offline --manifest-path src-tauri/Cargo.toml --test native_transport`.
El crate conserva los requisitos GTK/Wayland del proyecto al compilar; el módulo
no importa Python ni necesita Solaar, mouse, red o privilegios para sus pruebas.
El sandbox del agente puede requerir ejecución autorizada para sockets Unix locales.

N2: descubrir y validar interfaces Bolt, seleccionar propietario exclusivo,
identificar protocolo/dispositivo, descubrir features y realizar diagnóstico
estrictamente de solo lectura. Contrastar resultados reales sin controladores
concurrentes. La evidencia histórica del adaptador Solaar no acepta el backend nuevo.

Continuación: [N2 implementado](native-diagnostic.md), con aceptación de lecturas
físicas pendiente. Los límites de N1 anteriores describen su entrega original;
la apertura validada y el diagnóstico viven en el módulo `native` separado.

Continuación posterior: N2 ya está aceptado por Bolt. La sesión añade únicamente
una lectura fija del registro de emparejamientos del receptor, correlacionada por
selector, para encontrar el slot real sin emparejar/desemparejar. No es una API
genérica de registros. [N3](native-settings.md) añade setters escalares aislados;
la GUI todavía usa el worker anterior.
