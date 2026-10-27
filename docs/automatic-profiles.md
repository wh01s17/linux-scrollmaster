# Perfil actual y recuperación automática

27 de octubre de 2026. Sustituye el flujo de edición → aplicar → guardar →
habilitar reaplicación descrito en documentos anteriores.

## Comportamiento

- Cada cambio válido de DPI, ruedas, botones, atajos o gestos se confirma con el
  mouse y se guarda automáticamente en **Perfil actual**.
- Atajos y gestos agrupan las ediciones durante 400 ms. El estado compartido
  conserva el lote al salir del editor, fija la identidad al editar y serializa
  las peticiones. Las ruedas y DPI se envían al soltar el control; el DPI exacto,
  al confirmar el campo o pulsar Enter. No hay botones de aplicar estos cambios.
- **Guardar como…** conserva una copia con nombre. Rechaza nombres duplicados;
  las siguientes ediciones continúan en Perfil actual. Las copias anteriores no
  se sobrescriben. Si todavía no existe Perfil actual, se toma una lectura
  completa para crear tanto la copia como el perfil actual.
- Cargar un perfil confirma su aplicación y actualiza Perfil actual. El perfil
  se carga al abrir la aplicación y al reconectar el mouse, sin casilla optativa.
  Se conserva la selección armada de documentos anteriores; si no existe, se
  prefiere Perfil actual y luego el último perfil guardado para esa identidad.
  Un arranque sin mouse mantiene pendiente la recuperación hasta detectarlo.
- El proceso debe seguir abierto o en la bandeja para recuperar ajustes y
  ejecutar acciones de software. No se instala un servicio.
- Las reglas por aplicación conservan prioridad sobre el perfil general cuando
  corresponde su aplicación. Importar conserva su carácter local: no aplica ni
  activa lo importado durante esa operación.
- Velocidad de scroll del compositor, tema, idioma y preferencias de escritorio
  conservan su persistencia propia; no pasan a formar parte del perfil HID.

## Fronteras y coste

`ProfileManager.vue` presenta el estado de guardado y gestiona copias;
`ButtonAssignment.vue` emite asignaciones válidas; `GestureEditor.vue` valida el
borrador de gestos. `deviceState.ts` mantiene la cola, revisión, lotes y su
identidad; `App.vue` solo conecta los eventos. Los componentes reciben estado o
controles y emiten intención; no guardan archivos ni llaman HID directamente.

La GUI añade `autosave: true` a `set`, `remap`, `restore` y al primer `save`.
El servicio nativo guarda el snapshot de confirmación devuelto por `apply` en la
misma petición: no vuelve a consultar el dispositivo para guardar. Valida seis
ajustes y el catálogo completo de botones, conserva metadatos y usa la revisión y
publicación atómica existentes. El adaptador Python mantiene el mismo contrato
aprovechando la descripción que ya devuelve al finalizar la petición.

El nombre persistido `__current__` identifica un único perfil por dispositivo;
la GUI lo traduce como Perfil actual / Current profile. No admite renombrado o
eliminación: para conservar una variante se crea una copia. No cambia el schema
1–10. `save_copy` es una transformación local del documento, funciona sin mouse
y no abre HID. Las herramientas previas pueden seguir usando operaciones sin
`autosave`; esa compatibilidad no añade opciones al flujo de la GUI.

El primer snapshot de la GUI solicita `resume: true`. El host nativo y el bucle
Python retienen la intención si el mouse aún está ausente. Los sondeos posteriores
usan el perfil persistido. En Rust, si una lectura fresca ya coincide con el
perfil efectivo, se devuelve esa confirmación sin repetir preflight/apply y
sin setters. Una diferencia sigue exigiendo validación y lectura final reales.
No se ha medido una latencia física nueva; la evidencia de rendimiento es la
reducción de consultas comprobada en las pruebas.

## Fallos

Antes de escribir hardware se desarma el perfil anterior en disco. Una escritura
parcial o una confirmación inválida nunca publica ni arma un perfil nuevo. Si
el cambio quedó aplicado pero falla el guardado, el error lo indica y conserva
las confirmaciones; no se anuncia «guardado». Rust conserva el indicador de
publicación incierta si el error ocurre después del rename. No hay reintento
automático de una escritura de resultado desconocido.

En esa situación el sondeo normal no reactiva el perfil antiguo para revertir
el cambio. Una carga explícita o un nuevo arranque permite volver a la última
configuración guardada. Guardar como tras un error toma una lectura fresca,
en lugar de copiar silenciosamente un perfil actual anterior. Los límites de
100 perfiles y 128 KiB del documento siguen vigentes; alcanzar un límite produce
un error visible. No se promete persistencia ante cierre forzado antes de
confirmar un lote pendiente.

## Evidencia y aceptación pendiente

Se contrastaron README, plan y arquitectura con los servicios realmente usados.
Codebase Memory: nivel Verify, generación inicial 2026-10-26T19:27:30Z, sin huecos
registrados en las rutas consultadas; snippets y fuente directa confirmaron el
flujo. El índice es auxiliar, no prueba de exhaustividad.

Regresiones sin mouse: persistencia y recuperación; conservación de copias;
guardado local offline; conflictos de disco posteriores a la aplicación;
escrituras parciales; confirmaciones inválidas; lecturas incompletas; orden e
identidad de cambios consecutivos; agrupación de atajos; salida del editor;
restauración del botón al quitar la última acción; carga inicial sin opt-in;
primer guardado con nombre; sondeo sin consultas duplicadas. Se ejercitan los
SFC reales con el renderer en memoria, los servicios con estado sintético y la
persistencia en directorios temporales. Esto no equivale a prueba física.

Validación: `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml` y
`cargo test --offline --manifest-path src-tauri/Cargo.toml`.
Resultado: **842 pruebas aprobadas** (190 Python, 62 TypeScript/componentes y
590 Rust), compilación frontend y comprobación Rust correctas.
Las pruebas de sockets/procesos requieren salir del sandbox restrictivo, sin
mouse, root, red ni permisos hidraw.

Siguiente aceptación física: editar varias asignaciones desde la GUI, guardar
una copia, reiniciar la aplicación y apagar/encender el mouse comprobando que
vuelve Perfil actual. No se ejecutó esta prueba ni se modificó la instalación
del usuario en este incremento.
