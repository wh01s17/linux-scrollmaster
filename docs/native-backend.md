# Backend nativo: independencia de Solaar

Decisión de producto del 19 de octubre de 2026: Linux ScrollMaster tendrá
transporte hidraw e implementación HID++ propios en Rust. Es un objetivo obligatorio
y la prioridad de desarrollo. Estado: **N1 implementado y probado automáticamente**;
N2 aceptado en hardware Bolt; seis setters de N3 implementados con pruebas,
remapeo secundario implementado y roundtrip físico aceptado. Aceptación ampliada
y N6 pendientes; N5 parcial con [núcleo de perfiles](native-profiles.md).
Su [servicio síncrono](native-service.md) ya reconcilia ajustes/remapeos nativos
en pruebas sintéticas; el [dueño de perfiles](native-profile-owner.md) ya añade
arranque offline, lease y reconexión por identidad. El [worker JSON-lines](native-worker.md)
ya funciona en proceso separado con supervisor compartido. El [foco nativo](native-focus.md)
ya selecciona reglas y ofrece catálogo sin abrir HID. Guardián y sustitución del
worker Tauri pendientes; la evidencia nueva de foco es sintética.
La [compilación de botones de perfiles](native-profile-actions.md) ya conecta el
contrato persistido con el guardián finito, manteniendo los remapeos nativos como
requisitos previos y restaurando solo controles software. Las acciones en servicio
duradero tienen ahora una ruta optativa inicial; no se abrió hardware para este incremento.
El [propietario independiente de perfiles](native-profile-guardian.md) ya aloja
sesión/lease en un hijo persistente con protocolo cerrado y cierre confirmado,
conectado al servicio por `--serve-guarded`; esta ruta solo admite ajustes/remapeos nativos.
El modo separado [captura residente](native-profile-capture.md),
`--serve-guarded-capture`, añade motor/salida y restauración bajo el mismo lease.
El plan de botones queda fijo hasta cierre; transiciones, aceptación física y
sustitución Tauri siguen pendientes.
El [consumidor pasivo duradero](native-profile-events.md) ya atiende consultas/reposo
y cancela por pérdida del cliente, con memoria acotada y evidencia sintética.
N4 iniciado con recepción pasiva;
[motor de restauración](native-recovery.md) implementado con pruebas sintéticas;
[guardián pasivo independiente](native-guardian.md) probado con procesos y mouse
sintético; [teclado Rust y limpieza](native-keyboard.md) implementados con
comprobación voluntaria en estado cero. El [motor de acciones/gestos](native-actions.md)
tiene pruebas sintéticas y [entrega directa interna](native-action-delivery.md);
[captura finita optativa confirmada](native-capture.md) implementada y probada
sintéticamente. La [salida nativa de escritorio](native-desktop.md) ya está
implementada con preflight y pruebas sintéticas; aceptación física pendiente.
La biblioteca experimental aún no controla el mouse desde la app.

La versión actual utiliza Solaar 1.1.20 y un servicio Python. Sus perfiles,
acciones, gestos y recuperación son trabajo propio reutilizable, pero el acceso
al mouse depende de Solaar. Empaquetar sus módulos dentro de la app no cumple
el objetivo de independencia. Tampoco lo cumple sustituirlo por otro controlador
externo obligatorio.

## Arquitectura objetivo

Vue conserva presentación y estado; Tauri conserva la frontera validada. Un
servicio nativo separa perfiles/reconciliación, motor de acciones y backend HID++.
El transporte mantiene un único lector por dispositivo y entrega respuestas
correlacionadas y notificaciones sin lectores competidores. La salida Wayland e
integración del compositor siguen separadas del protocolo del mouse.

Se preservan contratos funcionales, schemas 1–10, identidad de dispositivos,
rutas y preferencias. Un cambio de backend no arma perfiles ni activa diversion.
Solo un backend posee el dispositivo; la transición no ejecuta comparaciones
simultáneas ni fallback silencioso a Solaar. Un backend experimental incompleto
no se activa por defecto.

## Etapas y aceptación

| Etapa | Entrega | Criterio para avanzar |
|---|---|---|
| N1, completada sin hardware | Inventario del contrato, transporte y codec Rust | 22 pruebas nuevas aprobadas, fixtures con procedencia, correlación, errores, timeouts, truncados, desconexión y notificaciones; [evidencia y límites](native-transport.md) |
| N2, aceptada por Bolt | Descubrimiento, identidad, batería, capacidades y lecturas; 29 pruebas | Slot real localizado, identidad BLE y seis valores confirmados en dos consultas con bytes contrastados y perfiles intactos; [evidencia](native-diagnostic.md) |
| N3, parcial | Seis setters y remapeo secundario implementados | DPI 1800→1850→1800 verificado y seis valores restaurados; lote validado, confirmación y errores parciales. [Ajustes](native-settings.md) y [remapeo Atrás→Adelante→Atrás aceptado](native-button-remapping.md); otros setters y aceptación ampliada pendientes |
| N4, parcial | [Recepción](native-events.md), [propietario](native-owner.md), [restauración](native-recovery.md), [guardián](native-guardian.md), [teclado](native-keyboard.md) y [motor](native-actions.md) con [entrega directa](native-action-delivery.md); [captura optativa confirmada](native-capture.md) y [escritorio nativo](native-desktop.md) probados sintéticamente | Escucha en reposo de 1000 ms y propietario/segunda lectura con 5001 ms aceptados; captura/acciones/recuperación nativas con evidencia sintética. Aceptación física pendiente; gestos y DPI deben conservar sus garantías |
| N5, implementado; aceptación física pendiente | [Perfiles](native-profiles.md), [servicio](native-service.md), [dueño](native-profile-owner.md), [worker](native-worker.md) y [foco/catálogo](native-focus.md) implementados para ajustes/remapeos nativos; guardián/sustitución Tauri pendientes | Schemas 1–10, HID++, reconexión y reglas/foco probados sintéticamente; worker probado con procesos reales y perfiles temporales; completar paridad con un solo propietario |
| N6, implementado; instalación limpia pendiente | Retirada de dependencias y distribución ([GUI y paquete](native-gui.md)) | Instalación limpia, build y ejecución sin Solaar; runtime sin Python; reglas hidraw propias acotadas; aceptación del flujo completo y desinstalación sin perder perfiles |

N1 usa hidraw directo con descriptor propio, E/S no bloqueante y `poll`; codec y
sesión no dependen de Solaar. La elección, inventario del contrato y límites están
en [transporte nativo](native-transport.md). N2 ya añade descubrimiento y diagnóstico
de solo lectura separado de `snapshot`, que puede reconciliar perfiles y escribir
hardware. Su [aceptación física Bolt](native-diagnostic.md) pasó después de cerrar
Solaar. N3 añade [setters escalares](native-settings.md) y
[remapeo secundario](native-button-remapping.md), sin activar el
backend en la GUI. N4 inicia [recepción pasiva](native-events.md); no habilita
diversion ni acciones. El [guardián pasivo](native-guardian.md) ya conserva el
lease y restaura al cerrar; su aceptación física sigue pendiente.
Las pruebas del backend nativo no deben importar Solaar ni usarlo como oráculo
necesario para pasar. Los fixtures no sustituyen la aceptación física.

Bolt es el primer transporte de aceptación por disponer de evidencia y dispositivo.
Bluetooth, varios receptores y otros modelos requieren validación propia; no se
declaran soportados por existir una rama de código o por funcionar con Solaar.

## Acceso y recuperación

El paquete final no dependerá de las reglas udev distribuidas por Solaar. Debe
incluir acceso mínimo a las interfaces necesarias del dispositivo soportado, con
revisión de alcance. No permisos 0666, acceso global a entrada, GUI root ni
cambios automáticos al escritorio. Preparar reglas es distinto de instalarlas;
su instalación y las pruebas físicas siguen el protocolo de permisos del proyecto.

La recuperación ante fallos es requisito de paridad. Antes de desviar botones,
deben existir consumidor, cancelación y restauración del reporting previo.
No se reintenta una escritura no idempotente tras timeout sin resolver su estado.
No se asume que cerrar un descriptor revierta el firmware. La persistencia mantiene
escrituras atómicas y evita armar automatismos por una importación o una migración.

## Definición de terminado

La independencia se declara solo al completar N1–N6 y registrar evidencia de uso
desde instalación limpia sin Solaar ni Python en runtime. Deben funcionar las
capacidades aceptadas del dispositivo: ajustes, perfiles, reconexión, botones,
gestos y recuperación, con límites de transporte explícitos. Las verificaciones
anteriores del backend Solaar no certifican el backend nuevo.

Estado al 21 de octubre de 2026: implementación completa y seleccionable con
`SCROLLMASTER_BACKEND=native` o el paquete `arch-native`. El backend por defecto
sigue siendo Solaar hasta completar el [protocolo de aceptación](native-acceptance.md).
