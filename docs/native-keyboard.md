# N4: salida de teclado Wayland desde Rust

Incremento del 20 de octubre de 2026, America/Santiago. `native/keyboard.rs` añade
una API Rust tipada para la biblioteca C propia, enlazada estáticamente. Ofrece
Alt/Ctrl mantenidos, edición, multimedia y atajos canónicos. El guardián puede
comprobar voluntariamente la creación/cierre del teclado en estado cero. El modo
separado [de captura optativa](native-capture.md) ya admite asignaciones y diversion
temporal confirmada, con pruebas sintéticas; la GUI sigue con
su backend anterior. No se realizaron pruebas sobre el escritorio o mouse reales.

## Frontera de salida

`WaylandKeyboard::open` exige usuario normal y crea un teclado virtual mediante
el protocolo autorizado por el compositor. C valida seat, manager y los índices
de modificadores del keymap virtual US; confirma estado inicial cero. No concede
permisos ni modifica el compositor. Si falta soporte o confirmación, falla.

El handle es privado, exclusivo y del mismo hilo (`!Send`/`!Sync`). No hay rutas
de bibliotecas, `dlopen`, importaciones Python ni acceso HID en esta capa. El build
genera una biblioteca estática C para Rust y sigue publicando atómicamente la
biblioteca compartida usada por el backend Python de transición. Se conservan
sus símbolos y firmas; se añade `scrollmaster_keyboard_close_checked` para comprobar
la liberación final sin romper el `close` anterior.

| API | Contrato |
|---|---|
| `set_held(HeldModifiers)` | Solo ninguno, Ctrl, Alt o ambos; no reenvía un estado confirmado idéntico |
| `tap(&KeyboardAction)` | Máscara, pulsación, liberación y restauración de los modificadores del propio teclado virtual |
| `KeyboardAction::parse` | Cuatro acciones de edición, seis multimedia o `custom:<modificadores>:<tecla>` canónico |
| `close` / destrucción | Un único cierre del handle; intenta liberar tecla pendiente y poner modificadores a cero |

Los atajos admiten 63 teclas: letras, dígitos, F1–F12, navegación y edición básica.
Los modificadores de combinación siguen el orden Ctrl/Alt/Shift/Super, sin
duplicados. El parser rechaza más de 80 bytes antes de descomponer la entrada.
No acepta keycodes arbitrarios, power/locks, puntuación, comandos ni acciones
de escritorio. El validador C comprueba de nuevo tecla y máscara.

Un fallo de estado o pulsación cierra la salida antes de devolver el error y
rechaza operaciones posteriores. No reabre ni repite automáticamente. El error
de la operación y una liberación no confirmada se conservan por separado. El
resultado del cierre es estable e idempotente, incluso tras fallo; no hay doble
liberación del puntero. Confirmación del compositor no equivale a aceptación por
la aplicación enfocada, y no se traduce según la distribución física del usuario.

## Esperas y limpieza

`native/wayland_wait.c` reemplaza roundtrips ilimitados por `sync`, preparación
de lectura, `poll`, recepción y dispatch sobre la conexión privada del teclado.
Cada lectura preparada acaba en recepción o cancelación, incluidas señales,
timeout y desconexión. Las confirmaciones tardías se rechazan.

El estado tiene hasta 2 s de confirmación; todas las fases de una pulsación
comparten **un plazo absoluto de 2 s**, sin reiniciarlo en cada roundtrip. El
cierre tiene su propio presupuesto de 2 s para liberar tecla pendiente y máscara.
La apertura comparte un presupuesto de confirmación; un fallo puede añadir el
intento acotado de limpieza. Las syscalls de conexión y la construcción del keymap
no constituyen garantías de tiempo real.

El bucle de lectura respeta el contrato de
[la API cliente de Wayland](https://wayland.freedesktop.org/docs/html/apb.html#Client-classwl__display):
preparar la lectura antes de esperar y cancelarla cuando no se consume el socket.
No hay lectores competidores sobre esa conexión.

## Integración con el guardián

`GuardianConfig::with_keyboard_check()` es optativo. La CLI añade
`--keyboard-check`; sin ese flag el guardián no abre ningún teclado. Con él, crea
un teclado en estado cero antes de anunciar `ready` y comprueba su cierre al
terminar. No pulsa teclas ni acepta acciones por IPC. El helper elimina de su
entorno `WAYLAND_SOCKET`, cuyo descriptor no se hereda, y abre una conexión
Wayland nueva en vez de reutilizar un fd que podría coincidir con su IPC privado.

La secuencia terminal es: cerrar propietario activo → liberar/cerrar teclado →
restaurar reporting → liberar lease → publicar evidencia. Un error de liberación
del teclado no evita restaurar el mouse y fuerza `completed:false`. El informe
`keyboard` distingue solicitud, apertura, confirmación de liberación y errores
de apertura/cierre. El cliente verifica que el ACK incluya la preparación de
teclado solicitada; no acepta `ready` cuando falla.

Ejemplo para aceptación futura, no ejecutado en este incremento:

```sh
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_guardian -- --observe hidraw7 --slot 2 --device <UNIT_ID> --seconds 5 --refresh --keyboard-check
```

El ejemplo también puede restaurar flags temporales registrados al finalizar;
se mantienen su selección explícita, guard de controladores y lease. No carga
perfiles, no cambia permisos ni instala servicios. El flag no acredita acciones
funcionales de botones ni habilita captura.

## Pruebas y límites

19 casos nuevos elevan la suite a **412 aprobadas**: 177 Python, 41 TypeScript y
194 Rust (136 biblioteca, 23 host, 35 integración). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`, suite
Rust completa, formato y diff. Las pruebas de sockets privados requieren salir
del sandbox, sin conectarse al compositor real.

Ocho casos Rust cubren catálogo, las 63 teclas con 16 combinaciones, entradas
inválidas, modificadores, conservación del estado y cierre idempotente ante
fallos/destrucción. Cuatro casos de procesos comprueban teclado ausente, orden
de limpieza, liberación sin confirmar, protocolo inválido y SIGKILL del cliente.
Se usan drivers de salida simulados y el mouse sintético del guardián.

Seis casos Python ejecutan el sincronizador C real con libwayland-client y un
peer Unix que simula únicamente el protocolo core: ACK normal/fragmentado,
respuesta tardía, presupuesto compartido, desconexión y plazo vencido. Otro caso
del probe C comprueba que el cierre no declare una liberación sin confirmación.
El probe anterior también verifica que la pulsación comparte el mismo plazo en
sus tres fases, que libera teclas y recupera la máscara anterior.

No se abrió un teclado en el escritorio real ni se verificaron físicamente la
autorización, recepción de acciones o tacto del mouse en este incremento. La
[captura optativa](native-capture.md) ya implementa asignaciones y activaciones
confirmadas; falta aceptación física específica. La [entrega directa](native-action-delivery.md)
ya tiene pruebas internas; `--keyboard-check` mantiene asignaciones vacías. Las acciones de escritorio
Hyprland requieren su propia salida; no pasan por este parser. N4 sigue parcial.
