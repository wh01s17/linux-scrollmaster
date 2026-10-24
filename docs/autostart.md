# Inicio automático voluntario

En Preferencias de Linux ScrollMaster, «Abrir Linux ScrollMaster al iniciar sesión» permite crear o retirar
una entrada XDG propia. Viene desactivada cuando no existe esa entrada. Al iniciar
sesión se abre la ventana normalmente; no empieza oculta ni se activa la bandeja.
La instancia única sigue evitando un segundo controlador. Desactivarlo evita el
próximo inicio por esta entrada y no termina la instancia que ya está abierta.

El escritorio debe procesar autostart XDG. Hyprland por sí solo no garantiza ese
procesamiento; Linux ScrollMaster no instala un lanzador, modifica Hyprland/Omarchy ni habilita
servicios. No se promete compatibilidad por haber escrito un `.desktop`. El sondeo,
los perfiles y las acciones siguen dependiendo de que Linux ScrollMaster permanezca abierta.
Los automatismos del mouse solo se recuperan si el usuario ya armó un perfil.

## Implementación

`src-tauri/src/autostart.rs` gestiona únicamente
`$XDG_CONFIG_HOME/autostart/io.github.wh01s17.linux-scrollmaster.autostart.desktop`, o la ruta
equivalente bajo `~/.config`. La GUI envía solo un booleano; no recibe permisos para
elegir rutas, ejecutables o argumentos. Rust obtiene su ejecutable actual y escapa
la ruta como un único argumento `Exec`, sin shell ni opciones adicionales.

No se duplica la preferencia en `desktop.json`: la entrada leída del disco es la
fuente del estado. Un cambio de ubicación se informa sin modificar nada. El botón
«Usar esta instalación al iniciar» permite actualizar expresamente una entrada
propia. Si la ubicación desaparece después de configurar, se puede desactivar la
entrada anterior. Después de recompilar/mover el binario puede ser necesario
reabrir Linux ScrollMaster antes de configurar la nueva ubicación.

Solo se modifica o elimina el formato canónico propio, marcado con `Managed by Linux ScrollMaster`
y la clave `X-LinuxScrollMaster-Autostart-Version`. Entradas ajenas,
editadas, corruptas, mayores de 8 KiB, enlaces simbólicos y archivos no regulares
se conservan y muestran error. La publicación usa un temporal 0600 con fsync;
una primera creación usa enlace duro sin sobrescribir el destino. Las actualizaciones
propias usan rename. Se lee de nuevo tras escribir o borrar. La serialización cubre
el IPC propio; no coordina editores externos concurrentes.

Una compilación de desarrollo (`pnpm tauri dev`, sin la feature `custom-protocol`)
carga la interfaz desde el `devUrl` de Vite (`localhost:1420`). Si se registra para
el inicio automático, al iniciar sesión Vite no está corriendo y la ventana muestra
«Could not connect to localhost: Connection refused». Por eso, con `tauri::is_dev()`
Rust rechaza activar o reubicar la entrada, no ofrece «Usar esta instalación» y solo
permite desactivar una entrada existente. La activación se hace desde el binario
instalado (`/usr/bin/linux-scrollmaster`) o desde una compilación `pnpm tauri build`.

El filesystem debe permitir esa publicación con enlaces duros. Se rechazan rutas
relativas, no UTF-8, con controles, `=` o `%`; el lector GIO no aceptó de forma fiable
el ejecutable con `%` en la prueba de compatibilidad. Se prueban espacios, Unicode,
comillas, dólar, backtick, barra inversa y punto y coma sin ejecutar el archivo.

Los comandos `autostart_preferences` y `set_autostart` trabajan fuera del mutex HID++
y del hilo gráfico. `src/desktopPreferences.ts` separa el estado de preferencias del
estado del mouse. La carga no escribe; ambos interruptores quedan bloqueados durante
una operación. Ante error se relee el estado real, incluso si un fsync falló después
de publicar, y se conserva el mensaje. Si falla esa lectura, se pide actualizar antes
de volver a modificar. `AutostartPreference.vue` solo presenta estado y emite intención.

## Verificación y aceptación pendiente

**233 pruebas aprobadas**: 170 Python, 17 del estado Vue, 5 de distancia, 4 de formato
de atajos, 8 de componentes Vue, 7 de estado de preferencias y 22 Rust. Son 18 casos
nuevos: 9 Rust, 7 del estado y 2 de componentes. Cubren activación/desactivación,
valores por defecto sin escrituras, aislamiento de perfiles/bandeja, permisos,
archivos ajenos/alterados, enlaces, límites, errores, rutas, actualización voluntaria,
concurrencia propia y lectura posterior a fallos. El parser GLib/GIO real valida
las entradas de prueba; no las ejecuta. Todas las escrituras de prueba son temporales.

`pnpm test`, `pnpm build`, `cargo check --offline`, `cargo test --offline`, formato
Rust y ejecutable debug integrado aprobados. La suite Rust completa requiere salir
del sandbox del agente para los sockets Unix temporales de instancia; no requiere
root, red remota, compositor ni mouse.

No se habilitó inicio automático real ni se abrió la GUI o modificó ninguna
preferencia del usuario durante este incremento. Pendiente: aceptación gráfica del interruptor
y un cierre/inicio de sesión en un escritorio que procese XDG, contrastando ventana,
instancia única y reaplicación voluntaria. Las aceptaciones previas de mouse, perfiles,
bandeja, reglas y diálogos siguen registradas en `plan.md`.

Referencias: [Autostart XDG](https://specifications.freedesktop.org/autostart/latest/) y
[formato de Exec](https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html).

El cambio de nombre añade una regresión de compatibilidad: suite actual de 234
pruebas, incluidas 23 Rust.
