# Acciones de espacios y ventanas en Hyprland

## Funciones y uso

Los laterales y las cinco acciones del editor de gestos ofrecen:

- Espacio anterior y espacio siguiente en el monitor activo, incluyendo espacios
  vacíos según el selector relativo `r-1`/`r+1` de Hyprland.
- Alternar pantalla completa en la ventana enfocada.
- Alternar maximizado en la ventana enfocada.

Elegir una acción en un lateral aplica la asignación, como las acciones de edición;
el cambio de espacio o ventana solo ocurre al pulsar el botón. Los gestos conservan
su borrador y botón Aplicar; ejecutan al soltar. Mantener pulsado no repite. El foco
se toma cuando se ejecuta, sin capturar una ventana o monitor al asignar el botón.
No se asignan acciones ni se guardan perfiles automáticamente.

Requieren Linux ScrollMaster abierta, teclado virtual Wayland disponible y Hyprland 0.55–0.57
con configuración Lua. Otras versiones se rechazan explícitamente hasta contrastar
su interfaz. Fuera de ese entorno las opciones aparecen deshabilitadas y se explica
el motivo. Los atajos y las funciones nativas conservan su validación independiente.
No se configura el escritorio, se instala software ni se conceden permisos nuevos.

## Contrato y recuperación

`bridge/actions.py` añade cuatro identificadores `desktop:*`. Las expresiones Lua
son constantes del catálogo; el JSON de GUI/perfiles nunca contiene código ni
argumentos del compositor. El servicio, motor y guardián validan el identificador.
No hay entrada genérica de shell, Lua, dispatcher, configuración o ruta de socket.

`bridge/desktop.py` conserva las consultas de foco/lista y añade lectura de versión,
una comprobación fija de capacidades y salida de acciones. La comprobación lee
`j/version` y evalúa una expresión constante que verifica la existencia de
`hl.dsp.focus` y `hl.dsp.window.fullscreen`; no los invoca ni cambia estado. Esto
detecta también una configuración hyprlang antigua en una versión reciente.

Antes de desviar botones se comprueba la capacidad real. El guardián vuelve a
comprobarla al ejecutar y resuelve la firma/ruta de sesión en cada conexión; no
conserva sockets ni versiones en caché. Si el compositor reinicia y Linux ScrollMaster conserva
una firma de sesión antigua en su entorno, se debe reabrir Linux ScrollMaster; no descubre
automáticamente otras sesiones. Envía solo uno de los cuatro dispatchers
fijos y exige una respuesta completa `ok`. Esto confirma que Hyprland aceptó la
petición, no que una ventana o espacio hayan cambiado de forma perceptible.

Cada intercambio tiene un plazo total de 1 s que incluye conectar, enviar y leer;
las respuestas de comprobación/acción se acotan a 4096 bytes. Una ejecución usa
hasta tres intercambios; el guardián tiene 5 s para confirmar. No se reintenta tras
timeout: una petición podría haberse ejecutado y repetirla alternaría el estado.

El guardián despacha IPC directamente sin sintetizar teclas ni cambiar la máscara
de modificadores. Si falla, cierra el teclado y restituye los controles registrados;
el worker conserva el aviso de recuperación. Se mantiene un solo lector HID++ y
el mecanismo previo de liberación, cancelación y restauración de reporting. Las
limitaciones ante compositor/hardware desconectados o fallo del propio guardián
siguen vigentes; no se promete restitución absoluta.

## Perfiles y evidencia

Schema 10 conserva estas acciones en laterales o dentro de gestos. Schemas 1–9
siguen siendo válidos y no se reescriben al leer. Guardar calcula la versión mínima;
restaurar, reconciliar, importar/exportar y reglas por aplicación conservan los
identificadores. Importar no arma perfiles y gestionar un archivo no ejecuta acciones.

**215 pruebas aprobadas**: 170 Python, 17 de estado Vue, 5 de distancia, 4 de formato
de atajos, 6 de componentes Vue y 13 Rust. Las 16 regresiones nuevas cubren catálogo,
versiones/capacidades, protocolo, timeout sin reintento, sesión nueva, cancelación,
ruta del guardián, preflight sin escrituras, lectura confirmada y roundtrip de
perfiles con dobles. Los componentes prueban opciones deshabilitadas y que cargar
una asignación no la ejecuta. Las pruebas regulares no usan el compositor real.

`pnpm test`, `pnpm build`, `cargo check --offline`, `cargo test --offline`, formato
Rust y ejecutable debug integrado aprobados. Se contrastaron estáticamente la
sintaxis Lua y el `ok` con Hyprland v0.56.2. La sesión anuncia 0.56.2 por consulta
de versión. La comprobación real de capacidades también aprobó: versión y funciones
Lua disponibles, sin invocar dispatchers. No se ejecutaron cambios reales de
espacios/ventanas ni del mouse y
no se modificaron perfiles reales o archivos de configuración.

Pendiente: aceptación GUI y física de las cuatro acciones, aplicación en foco,
reconexión, perfiles schema 10 y recuperación en la sesión real. Las aceptaciones
anteriores pendientes (schema 8/9, bandeja, reglas y diálogos) se conservan.

Fuentes primarias: [dispatchers Lua y selectores](https://wiki.hypr.land/Configuring/Basics/Dispatchers/),
[IPC](https://wiki.hypr.land/IPC/) y
[implementación v0.56.2 de eval/dispatch](https://github.com/hyprwm/Hyprland/blob/v0.56.2/src/debug/HyprCtl.cpp).
