# N4: motor nativo de botones y gestos

Incremento del 20 de octubre de 2026, ampliado el 21, America/Santiago. `native/actions.rs`
implementa las reglas de acciones en Rust y su salida hacia `WaylandKeyboard`.
Las pruebas usan salida simulada y frames HID++ escritos a mano. El motor ya tiene
[entrega directa interna desde el consumidor](native-action-delivery.md); la
comprobación del guardián usa asignaciones vacías. El modo separado [de captura](native-capture.md)
admite asignaciones explícitas y activaciones confirmadas. No está conectado a la GUI. No se
habilitó diversion, no se emitieron teclas en el escritorio y no se abrió el mouse.
N4 sigue parcial y la aplicación conserva Solaar/Python.

## Contrato

`ActionBindings::parse` acepta un objeto con claves opcionales `83`, `86`, `195`.
Los laterales admiten `modifier:ctrl`, `modifier:alt` o el catálogo cerrado de
[acciones de teclado](native-keyboard.md) y [escritorio nativo](native-desktop.md).
El CID 195 admite un objeto de gestos
con `type:gestures`, `threshold`, `actions` y `units` opcional. Las cinco claves
de acciones son `up`, `down`, `left`, `right`, `click`; cada valor es una acción
de teclado, escritorio o `null`. Sin `units` se conserva la interpretación histórica en counts.

Los rangos son 20–2000 counts o 10–200 décimas de milímetro. Se rechazan campos
desconocidos, valores no enteros, CIDs primarios y comandos arbitrarios.
Este parser cubre las acciones cerradas de las asignaciones existentes; no es
un lector de perfiles ni implementa schemas 1–10. Omitir una clave elimina su
acción de software; no modifica el mapping ni los flags del firmware.

`NativeActions::new` recibe propiedad exclusiva de un teclado ya autorizado y
confirma sus modificadores a cero antes de asumir el estado inicial. No descubre
hardware ni abre otro lector. Sus métodos reciben listas actuales de botones y
movimientos, con reloj `Instant` propio; no aceptan comandos ni keycodes arbitrarios.

| Entrada | Comportamiento |
|---|---|
| Primera lista de botones | Sincroniza y suprime los mantenidos hasta que se suelten; no emite acciones |
| Nueva pulsación lateral | Un atajo por flanco, sin repetición por informes idénticos |
| Alt/Ctrl mantenidos | Unión de botones activos; soltar uno no libera el modificador compartido por otro |
| Pulsaciones simultáneas | Actualiza modificadores antes de emitir los atajos, en orden CID 83/86 |
| `replace_bindings` | Suprime los botones mantenidos cuya asignación cambió; libera modificadores anteriores; una asignación idéntica conserva el gesto |
| `cancel` | Cancela gesto, libera modificadores y exige otra sincronización completa |
| `close` / destrucción | Cierre único de salida; intenta liberar teclas y modificadores; resultado de cierre estable |

Una lista con duplicados, CIDs desconocidos o más de tres controles cierra la
salida. Un fallo de emisión también termina el motor antes de devolver el error.
No reabre ni repite operaciones inciertas. `last_error` conserva el primer error
de operación; `close` conserva por separado una liberación sin confirmar. Las
pruebas inyectan también reloj regresivo y exigen cierre sin producir un click.

## Gestos y DPI

El gesto empieza con una nueva pulsación de 195 y se consume antes de emitir
al soltar. Acumula XY con saturación ±1 000 000, conserva el pico y fija la primera
dirección que alcanza el umbral y domina el otro eje por 6:5. El retorno al origen
no se convierte en click. Si el pico queda por debajo del umbral se reconoce click;
una diagonal sin dominancia se informa como ambigua y no emite acción.

Los gestos de más de diez segundos se cancelan, con o sin informes de movimiento.
El límite de diez segundos es inclusivo. Una dirección con acción `null` produce
feedback sin teclado. `GestureFeedback` conserva secuencia y dirección; dirección
ausente significa ambigüedad. Ese dato acredita reconocimiento, no recepción por
la aplicación enfocada ni éxito de emisión.

La distancia física calcula `ceil(threshold * dpi / 254)` sobre el acumulado,
sin redondear cada paquete. La activación de esa configuración exige una lectura
DPI válida, nunca un valor supuesto. Cambiar o perder DPI cancela el gesto físico
actual y suprime 195 hasta soltarlo; recuperar DPI no reanuda el gesto anterior.
Una nueva pulsación sin DPI falla y cierra. Los gestos en counts conservan su
umbral al cambiar DPI. La API tipa DPI como `Option<u16>` y rechaza cero.

## Evidencia y siguiente integración

23 pruebas nuevas cubren validación, sincronización, orden, repetición, modificadores
compartidos, reasignación, cancelación, las cinco direcciones, diagonales, jitter,
timeout, saturación, DPI y errores de salida/limpieza. Una prueba conecta el
decodificador real a la máquina con frames 1B04 sintéticos, incluida una respuesta
con software ID distinto de cero y un movimiento negativo. No es evidencia física.

La suite al introducir el motor aislado tuvo **435 aprobadas**: 177 Python, 41 TypeScript y 217 Rust
(159 biblioteca, 23 host, 35 integración). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`, suite
Rust, formato y diff. Se corrigió además una carrera de la prueba IPC de EOF:
ahora espera dentro del plazo normal cuando otro fixture conserva brevemente un
descriptor entre fork y exec. No cambia el comportamiento del canal de producción.

La [conexión interna](native-action-delivery.md) ya entrega notificaciones actuales
desde el único `ReportConsumer`, incluye getters, valida capacidades/atribución raw
XY y cierra acciones antes de restaurar. **No se debe reproducir el JSON histórico
del observador para ejecutar acciones.** La [captura optativa](native-capture.md)
ya configura asignaciones y confirma cada activación con guardián/lease preparados.
Sus eventos durante ACK/readback se registran con salida bloqueada. La [salida de
escritorio](native-desktop.md) se añadió el 21 de octubre; aceptación física sigue
pendiente. IPC: estado/refresh/cierre.
