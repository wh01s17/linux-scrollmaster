# Botones: asignación nativa HID++

**Registro de la primera fase.** Alt/Ctrl ya son funcionales y verificados con el
usuario; el estado vigente y la recuperación de diversion están documentados en
[held-modifiers.md](held-modifiers.md). Las limitaciones siguientes describen el
incremento nativo anterior y no sustituyen esa actualización.

Incremento posterior al hito de fiabilidad; sesión del 17 de octubre de 2026
(lecturas con fecha UTC del 18). No se activó diversion ni se cambiaron permisos
o configuraciones del sistema.

## Alcance y estado

Implementadas la lectura de capacidades/asignaciones, escritura de funciones
nativas con confirmación, controles GUI y persistencia en perfiles versión 2.
La lectura del catálogo está verificada en el mouse real. La escritura y aceptación
física desde GUI están pendientes de la prueba con el usuario al redactar este registro.
Gestos direccionales, atajos e inyección de eventos siguen sin implementar.

Esta es una ruta anterior al motor de eventos del hito B: el firmware puede
reasignar un control a la tarea nativa de otro. No requiere capturar pulsaciones
ni sintetizar entrada. No se promete que la función original del botón de gestos
sea un motor de gestos configurable en Linux.

## Evidencia de lectura real

`python tests/hardware_observe.py`, a 01:53:08 UTC:

- Dispositivo <UNIT_ID>, MX Master 3S por Bolt, batería 40 %.
- Atrás: CID 83; asignado a 83.
- Adelante: CID 86; asignado a 86.
- Botón de gestos: CID 195; asignado a 195.
- Cada control anunció siete destinos: clic izquierdo, clic derecho, clic central,
  Atrás, Adelante, función nativa del botón de gestos y cambio de modo de rueda.
- Se mantuvieron 1850 DPI, SmartShift 10, modo dentado, alta resolución true,
  inversión vertical false y lateral true.
- El archivo del usuario seguía en versión 1 y `active` estaba vacío en esta
  observación. No se rearmó ni se sobrescribió automáticamente.

No confundir esta lista con una especificación fija para todos los modelos.
Las opciones de destino provienen de `remappable_to` del dispositivo. Solo se
permiten como origen los tres controles secundarios anteriores; los clics
principales no se pueden cambiar por la operación `remap`.

## Implementación y garantías

- Todas las referencias a interfaces Solaar continúan dentro de
  `bridge/solaar_adapter.py`; no hay dependencias nuevas.
- Capacidad y opciones se conservan por conexión; cada asignación actual se
  consulta de nuevo mediante getCidReporting de la feature 1B04, función 0x20.
- La propiedad `mapped_to` de Solaar puede usar caché y sustituir un fallo por
  valores de fábrica. Por ello Linux ScrollMaster valida la respuesta real: longitud, CID
  coincidente y destino anunciado. Un fallo no se presenta como lectura válida.
- La operación `remap` valida fuentes, tipos y opciones del lote completo antes
  de escribir. Se bloquean controles interceptados, persistently diverted, raw XY
  y raw wheel. No se llama a set_diverted ni a set_rawXY_reporting.
- Se usa la reasignación nativa de Solaar y después se lee el reporting de nuevo.
  Una respuesta no coincidente produce error e invalida los controles en Vue.
- El host conserva serialización y límites existentes. No hay un segundo lector
  de hidraw ni un stream de eventos que compita con respuestas HID++.
- Como los ajustes escalares, una reasignación manual desarma el perfil automático
  antes de escribir. Si falla el disco no se realiza la escritura.
- Los lotes no son transacciones: errores informan de cambios anteriores
  confirmados; una escritura cuyo readback falla puede haber llegado al firmware.

## Compatibilidad de perfiles

Se acepta schema 1 sin reescribirlo al leer. Los perfiles anteriores que solo
contienen `settings` restauran esos ajustes y dejan las asignaciones actuales
de botones tal como están. No se les inventan asignaciones por defecto.

Al guardar un perfil con catálogo completo se escribe schema 2 e incluye
`buttons: {"83": 83, "86": 86, "195": 195}` según los valores leídos. El esquema
2 también admite perfiles antiguos sin `buttons`. Un error de lectura de botones
impide sobrescribir un perfil con datos parciales.

Restaurar/reconciliar un perfil con botones valida primero sus capacidades antes
de cambiar los ajustes escalares. La lista de reconciliación usa `button:83`, etc.,
para distinguir asignaciones efectivamente escritas. La migración se produce
solo al guardar; no hay edición silenciosa del perfil del usuario.

## Regresión de versión encontrada al crear los primeros commits

El primer intento de lectura devolvió «Solaar c42c418 no es compatible». El
`__init__.py` del paquete instalado ejecuta `git describe --always` con
`cwd=sys.path[0]`. Una vez que Linux ScrollMaster tuvo commits, la variable `solaar.__version__`
pasó a identificar el commit de Linux ScrollMaster, no la versión instalada de Solaar.

Se corrigió el gate para usar `importlib.metadata.version('solaar')`, que devuelve
1.1.20. Si no puede verificar los metadatos, falla explícitamente. Se conserva
la restricción de versiones probadas. Una regresión automatizada reproduce el
hash ajeno en `__version__`; la lectura física posterior confirmó la corrección.

## Pruebas de la primera fase

60 pruebas: 44 Python, 13 estado Vue y 3 Rust. Los nuevos casos cubren capacidades,
lecturas sin caché, respuestas ausentes/truncadas/de otro CID, protección de clics
principales, tipos, validación de lote, controles interceptados, readback fallido,
schema 1/2, fallos de disco, recuperación de perfiles con botones y mensajes GUI.
Los dobles solo verifican lógica y contrato; no sustituyen la aceptación física.

Compilaciones ejecutadas: `pnpm build`, `cargo check --offline`,
`cargo test --offline` y `pnpm tauri build --debug --no-bundle`.

## Pendientes de este incremento

Alt/Ctrl se añadieron primero como opciones deshabilitadas. Se sustituyeron por
soporte real con consumidor, guardián y teclado Wayland; el usuario confirmó ambas
teclas y su liberación. Ver [held-modifiers.md](held-modifiers.md).

- Verificar una reasignación desde GUI y lectura independiente en el mouse real.
- Confirmar físicamente la acción en una aplicación con historial de navegación.
- Devolver el botón a su función original y confirmar el retorno.
- Guardar un perfil separado con botones, restaurarlo y comprobar su recuperación
  al reconectar con participación del usuario, sin sobrescribir «Mi perfil».
- Investigar el consumidor de eventos y recuperación de diversion para el motor
  de gestos/atajos, manteniendo las tareas restantes del hito B en `plan.md`.

Fuentes: implementación local de Solaar 1.1.20 en
`logitech_receiver/hidpp20.py` (ReprogrammableKeyV4, get/setCidReporting,
KeyFlag, MappingFlag, KeysArrayV4) y [capacidades oficiales de Solaar](https://pwr-solaar.github.io/Solaar/capabilities/).
