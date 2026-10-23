# N5: foco y catálogo de aplicaciones en Rust

Incremento del 21 de octubre de 2026, America/Santiago. El worker Rust ya consulta
clase activa y catálogo Hyprland, y entrega al servicio el foco observado para
seleccionar reglas. **N5 sigue parcial; la GUI conserva Solaar/Python.** Pruebas
con compositor/dispositivo sintéticos, sin abrir hardware, sesión o perfiles reales.

## Separación

| Código | Responsabilidad |
|---|---|
| `native/focus.rs` | Consultas fijas, validación de respuesta y descripción sin datos de ventanas |
| `native/desktop.rs` | Transporte Unix compartido, credenciales y plazos absolutos |
| `profiles/model.rs` | Validador común de clase y selección de perfil efectivo |
| `worker.rs` | Obtener foco confiable antes de snapshot; adjuntar desktop; aceptar applications |
| `service.rs` | Reglas generales/manuales y listado de perfiles sin HID para applications |

`DesktopSource` es un adaptador de confianza, no un campo del protocolo. El
binario usa `HyprlandFocus::environment`; `serve_with_desktop` permite inyectar
el adaptador explícitamente. `serve` utiliza `NoDesktop` para embedding offline.
Construir cualquiera no consulta sockets ni cambia configuración.

## Consultas y privacidad

Snapshot ejecuta únicamente `j/activewindow`. Applications usa esa consulta y
después `j/clients`, sobre la misma ruta resuelta. Cada nueva petición vuelve a
resolver XDG_RUNTIME_DIR/HYPRLAND_INSTANCE_SIGNATURE, con las validaciones ya
existentes de ruta y SO_PEERCRED del usuario. No busca otras instancias, ejecuta
hyprctl/shell, lee socket2 ni envía versión, probes Lua o dispatchers.

El transporte comparte un plazo absoluto de **1 s para toda la descripción**,
incluidos ambos intercambios y validación. Las respuestas admiten **1 MiB cada
una**, hasta EOF; fragmentación y goteo no reinician el plazo. Se descartan
resultados tardíos, también al volver de un adaptador inyectado. Esto no hace
cancelable cualquier callback arbitrario ni garantiza tiempo real ante syscalls
bloqueadas. El servicio mantiene sus 30 s de hardware después de la lectura de
foco y el supervisor exterior conserva 45 s.

La respuesta activa debe ser objeto; clase ausente, nula, no textual o inválida
representa ausencia de foco usable. El catálogo debe ser array de objetos; los
elementos con clase inválida se omiten y un elemento no objeto invalida el
catálogo. Ambos usan el validador del modelo de perfiles: 1–256 caracteres,
todos >= U+0020. Se conservan espacios, Unicode y mayúsculas exactamente.
No se inventa identidad a partir de initialClass, título, PID u otro campo.

La descripción expone solo available/name/active_application y, cuando se pide,
applications. Los IDs se deduplican por igualdad exacta y se ordenan por
minúsculas Unicode con desempate exacto; no se aplica casefold completo ni se
normalizan IDs para comparar reglas. Títulos, PID, direcciones y nombres de
documentos no se copian a la respuesta ni a mensajes de error. El JSON recibido
sí se analiza en memoria antes de extraer clases; no es un parser de campos en
streaming. El límite de transporte acota ese material.

Ante JSON, forma, credenciales, conexión, tamaño o plazo inválidos se devuelve
available:false, active_application:null y error. Si falla el catálogo se
descarta también el foco obtenido antes en esa descripción; no se anuncia
éxito parcial. La siguiente petición comienza de nuevo y puede recuperarse.
Available acredita consultas de foco; no acredita soporte de acciones Lua.

## Servicio y reglas

El worker consulta foco fresco antes de cada snapshot y entrega solo la clase
observada a `ProfileService::handle`. La UI no puede enviar application para
snapshot ni elegir socket/consulta. Regla y clase se comparan de forma exacta;
la diferencia de mayúsculas no coincide. Con general armado se aplica la regla
coincidente; sin coincidencia/foco o ante error se usa el general. Sin general
armado no se aplica ninguna regla. Se conservan perfiles, reglas y activación
persistidos; cambiar foco no los reescribe.

Set/remap/restore siguen desarmando el general antes del primer setter, por lo
que el siguiente foco no revierte una edición manual. Las asignaciones de
software continúan rechazadas hasta integrar guardián/salidas. Una consulta de
foco exitosa no autoriza diversion ni salta validaciones de capacidades HID.

`{"op":"applications"}` admite device opcional y devuelve perfiles/activación
más desktop. Primero valida/carga el archivo; si falla, no consulta el compositor.
No adquiere lease/HID, reconcilia ni escribe el archivo, incluso con perfil
general armado. Un fallo del compositor conserva ok:true con desktop no
disponible, para permitir gestión offline. Las demás operaciones locales no
consultan escritorio ni adjuntan un foco cacheado. Snapshot aún puede consultar
foco antes de que el servicio detecte un error de perfiles/identidad.

No hay polling autónomo: el llamador debe enviar snapshots. El foco es una
observación previa al trabajo HID y puede cambiar durante getters/setters; la
siguiente consulta reconcilia. No se promete cambio instantáneo/transaccional.
GNOME/KDE/X11 y un catálogo sin Hyprland no están implementados.

## Evidencia y continuidad

24 pruebas nuevas: doce del adaptador/socket Unix; nueve conectan foco confiable,
worker, perfiles y dispositivo sintético; tres arrancan el binario de producción
contra un compositor simulado y perfiles temporales. Cubren formas/privacidad,
identidades Unicode, consultas fijas, cambio de sesión, fragmentación, límite,
presupuesto conjunto, ausencia/fallo/recuperación, reglas exactas, regreso al
general, desarmado manual, resultados tardíos, rechazo de foco por IPC y listado
sin adquirir hardware ni alterar perfiles. No sustituyen aceptación física.

**654 aprobadas**: 178 Python, 41 TypeScript y 435 Rust (360 biblioteca, 20 host,
44 integración y once CLI). Pasan pnpm test/build, cargo check/test offline con
--all-targets, formato y diff. Los sockets/procesos se ejecutaron fuera de la
restricción del sandbox. Auditoría: 123 archivos de código propios, ninguno con
más de 1000 líneas; máximo 977, sin excepción nueva.

Siguiente: integrar perfiles de acciones/gestos con guardián y recuperación;
cerrar paridad de respuestas y sustituir el worker Tauri con un solo propietario.
Aceptación física nativa de foco/captura/reconexión pendiente; N6 retirará
Solaar/Python tras paridad e instalación limpia.
