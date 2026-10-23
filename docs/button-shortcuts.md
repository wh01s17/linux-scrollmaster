# Acciones de edición desde los botones laterales

Incremento posterior a Alt/Ctrl mantenidos. Se conserva la interfaz e identidad
Linux ScrollMaster, el backend Solaar 1.1.20 y el guardián ya verificado. No hay dependencias
nuevas, instalaciones ni cambios en permisos/configuración del sistema.

## Funcionalidad

Los dos laterales ofrecen cuatro acciones adicionales:

| Acción | Combinación enviada |
|---|---|
| Copiar | Ctrl+C |
| Pegar | Ctrl+V |
| Cortar | Ctrl+X |
| Deshacer | Ctrl+Z |

Se ejecutan una vez al presionar, sin repetir mientras se mantiene el botón ni
repetir al soltarlo. Las acciones dependen de los atajos de la aplicación activa;
los nombres de la GUI muestran la combinación. No se lee ni escribe directamente
el portapapeles y no se ejecutan comandos o texto proporcionado por el usuario.

Alt y Ctrl mantenidos siguen disponibles. Los clics principales permanecen
protegidos y las acciones nuevas no se asignan automáticamente. El botón de
gestos conserva su catálogo nativo: no se presenta un motor direccional terminado.

## Implementación

- `bridge/actions.py`: catálogo cerrado con acciones, etiquetas y códigos de la
  distribución XKB usada por el teclado virtual propio. Modelo y salida comparten
  ese catálogo para evitar aceptar comandos o destinos inventados.
- `ModifierKeys`: calcula flancos de pulsación a partir del conjunto real de CIDs.
  Ignora duplicados y deja de ejecutar una acción cambiada con el botón mantenido
  hasta que se suelte y vuelva a presionar. Mantiene el estado de modificadores.
- El mismo consumidor HID++ entrega los eventos; no se abre un lector competidor.
- El guardián admite un comando `tap` limitado al catálogo, sin código libre.
- `native/keyboard.c`: envía modificadores Ctrl, tecla abajo, tecla arriba y
  restaura la máscara de modificadores que había antes de la acción. Mantiene
  registro de la tecla pendiente para intentar liberarla también durante cleanup.
  El C limita la salida a las cuatro teclas del catálogo; no es una API para
  escribir texto arbitrario.
- Si falla el motor idle, el backend se cierra y se inicia recuperación. El error
  se conserva como warning para el siguiente snapshot. La GUI elimina un mensaje
  de éxito antiguo al recibir ese aviso; no oculta el cambio de estado.
- El campo «Probar Copiar y Pegar» recibe eventos reales y ejecuta edición normal
  de un textarea. No emula resultados a partir de la asignación seleccionada.

## Persistencia

Schema 4 añade `shortcut:copy`, `shortcut:paste`, `shortcut:cut`, `shortcut:undo`.
Los schemas 1/2/3 siguen válidos y no se migran al leer. Guardar un perfil nuevo
con atajos incrementa la versión sin eliminar los perfiles existentes ni degradar
su versión. Un perfil viejo sin botones sigue sin modificar botones al restaurar.

Los modificadores conservan su semántica de schema 3. El guardado/restauración
de acciones schema 4 se probó automáticamente; su aceptación física de reconexión
todavía es una tarea separada.

## Evidencia

- Usuario confirmó **«Copiar y Pegar funcionan»** tras asignarlos desde GUI y
  copiar/pegar texto con los botones reales en el campo de prueba.
- Usuario confirmó Cortar/Deshacer y devolvió ambos botones a sus funciones
  originales. Las cuatro acciones quedaron verificadas en el campo GUI nativo.
- Lectura independiente 03:23:15 UTC: Atrás 83→83, Adelante 86→86 y botón de
  gestos 195→195, todos disponibles y sin diversion ajena. Perfil «Mi perfil»
  intacto, schema 1 y active vacío. DPI 1850, SmartShift 10, modo 2, alta resolución
  false; no se sobreescribió con el true histórico del perfil.
- **99 pruebas automatizadas PASS: 81 Python, 15 estado Vue, 3 Rust.** Incluyen
  flancos, ausencia de repetición, convivencia con modificadores, reasignación
  mientras se mantiene el botón, whitelist, protocolo del guardián, schema 4 y
  conservación visible del error después de recuperar el backend.
- `pnpm build`, Tauri debug integrado, cargo check/test offline, fmt y sintaxis
  de PKGBUILD ejecutados. No se instalaron paquetes.

## Pendientes y límites

- Validar físicamente guardar/restaurar/reconectar un perfil schema 4.
- Probar fallo/desconexión justo durante un atajo y observar liberación en la app.
- Hyprland/Wayland es el entorno físico probado. No prometer X11 ni otros
  compositores por el mero hecho de existir un protocolo de salida.
- Este incremento describía un catálogo cerrado. Posteriormente se añadieron
  gestos, perfiles por aplicación y [atajos personalizados](custom-shortcuts.md).
  Las macros siguen pendientes; la aceptación de cada función se registra por separado.

Referencia del protocolo de salida: [virtual-keyboard-unstable-v1](https://github.com/atx/wtype/blob/master/protocol/virtual-keyboard-unstable-v1.xml).
Para la recuperación del dispositivo y pruebas reales de cierre/SIGKILL consultar
[held-modifiers.md](held-modifiers.md).
