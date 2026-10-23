# N4: entrega directa al motor y cierre de salida

Incremento del 20 de octubre de 2026, America/Santiago. El consumidor de la sesión
ya entrega eventos tipados al motor de acciones en el mismo hilo, tanto en reposo
como durante getters. La conexión al guardián es interna: **`--keyboard-check`
construye el motor con asignaciones vacías**, conservando su comprobación en estado
cero. El modo separado [de captura](native-capture.md) ya fija asignaciones y
confirma reporting al arrancar; ningún mensaje IPC permite cambiarlos.
La GUI sigue con Solaar/Python. No se abrió hardware ni compositor reales.

## Propiedad y entrega

`events::Event` sale directamente del decoder validado de slot/feature/software ID
y capacidades; el JSON se genera aparte para evidencia. `EventSink` define entrega,
preparación de consultas, aceptación del snapshot y cierre. `SinkHandle` comparte
un único recurso entre captura y guardián en el mismo hilo, conservando el resultado
del primer cierre, incluido un panic recuperable. No duplica HID ni teclado.

La API pública `PassiveEventOwner::open` sigue sin salida. La variante interna
`open_with_sink` valida identidad/catálogo, entrega el snapshot al motor y adjunta
el consumidor. La FIFO acumulada durante ese setup se conserva en el historial,
pero **no se entrega a acciones**. El primer estado de botones recibido después
sincroniza el motor y suprime los mantenidos hasta soltarlos. No se reproduce el
historial en consultas de estado, refresh ni recuperación.

Después del setup, cada notificación decodificada se entrega sincrónicamente desde
el mismo `ReportConsumer` de `Session`. Los getters conservan ese consumidor.
No hay cola de acciones, segundo lector, replay ni reintento de pulsaciones
inciertas. Las reglas de límites y plazos de sesión siguen vigentes; una salida
lenta no amplía el plazo del getter o del sondeo.

La evidencia añade `sink_attached`, `sink_deliveries` y `sink_release_error`.
Cada evento lleva `delivered_to_sink`; significa intento de entrega al motor,
no confirmación de una tecla ni recepción por la aplicación enfocada. Los errores
o panics de callback cierran la captura con `output-error` y conservan evidencia
parcial. El error se propaga también al informe terminal del guardián.

## Capacidades, raw XY y DPI

El motor contrasta sus CIDs configurados con el catálogo real: presencia única,
control físico/reprogramable/divertable y capacidad raw XY para gestos. No escribe
mapping ni reporting. Raw XY carece de CID en el wire; el evento tipado conserva
los controles mantenidos que podrían originarlo según sus capacidades. El motor
solo acepta movimiento con fuente posible única 195. Una combinación ambigua
cancela el gesto, evitando convertir su liberación en un click artificial.

Antes de cada refresh se invalida la lectura DPI del motor. Eso cancela los
gestos físicos actuales y suprime nuevas pulsaciones de 195 durante la consulta,
hasta soltarlas. Los atajos laterales, modificadores y gestos en counts siguen
recibiendo eventos. Tras validar identidad y la misma configuración de botones,
el propietario entrega el snapshot actualizado; solo entonces confirma el refresh.
Una lectura DPI ausente o inválida impide continuar gestos físicos; no se supone
un DPI. El getter conserva la regla HID++ existente que permite el valor por
defecto anunciado cuando el sensor informa cero. Una consulta de estado en caché
no invalida DPI ni ejecuta eventos.

## Terminación y recuperación

Cierre, cancelación, error, saturación o destrucción de la captura cierran la
salida. El motor puede liberar inmediatamente al detectar su propio fallo, antes
de devolverlo a la captura. En todos los casos el guardián termina el préstamo de
la sesión, recoge la confirmación de cierre del mismo recurso y lo destruye antes
de restaurar reporting, con el lease todavía retenido. Un fallo de liberación
no impide la recuperación del mouse y fuerza un resultado sin éxito.

La liberación no se repite desde distintos propietarios del handle. El protocolo
sigue limitado a estado/refresh/cierre; tampoco una operación IPC inválida puede
emitir una tecla. La observación no habilita diversion; la captura lo hace de forma
explícita y confirmada. No se añaden servicios, permisos o configuraciones del escritorio.

## Evidencia y pendiente

17 casos nuevos: trece de sesión/decoder/motor y cuatro de procesos del guardián.
Cubren FIFO inicial, entrega en reposo/getters, estado compartido, cancelación por
DPI, atribución ambigua de raw XY, capacidades, errores/panics de callbacks y cierre,
límites, desconexión, Drop, EOF, SIGKILL del cliente, plazo y protocolo inválido.
Las asignaciones de las pruebas existen únicamente en fixtures con salida y mouse
simulados; no acreditan acciones físicas ni autorización del compositor real.

Suite tras añadir captura: **486 pruebas aprobadas**: 177 Python, 41 TypeScript y 268 Rust (204 biblioteca,
23 host, 35 integración, seis CLI). Pasan `pnpm test`, `pnpm build`, check Rust offline de
todos los targets, suite Rust completa, formato y diff. Los sockets Unix privados
de las pruebas de procesos se ejecutan fuera del sandbox; no requieren mouse.

La [captura optativa](native-capture.md) ya añade configuración explícita, registro,
ACK/lectura posterior y pruebas de eventos intercalados. Durante esas confirmaciones
la salida permanece bloqueada; el consumidor y recuperación se preparan antes de
escribir. La [salida de escritorio](native-desktop.md) ya está implementada con
pruebas sintéticas. Faltan aceptación física, integración Tauri/perfiles y retirada
Solaar/Python.
N4 sigue parcial; no activar un backend incompleto por defecto.
