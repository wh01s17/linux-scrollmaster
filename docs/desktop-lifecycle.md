# Instancia única y bandeja opcional

Linux ScrollMaster conserva un único controlador por sesión mediante un bloqueo de archivo
mantenido durante toda la vida del host. Un segundo inicio envía únicamente `show`
al socket privado y termina antes de crear el worker o una segunda GUI. Si el
propietario no responde no se abre otro controlador. El bloqueo se libera por el
kernel incluso tras un fallo; un nuevo propietario retira el socket obsoleto.

El socket está bajo `$XDG_RUNTIME_DIR/linux-scrollmaster/`, directorio 0700 cuyo propietario se
verifica. No se aceptan directorios públicos ni enlaces simbólicos. Las lecturas
IPC se acotan a cuatro bytes y 200 ms; no existe ejecución de comandos. Requiere
Rust 1.89+ para el bloqueo estándar `File::try_lock`, sin otra dependencia de bloqueo.

La preferencia «Continuar en la bandeja al cerrar la ventana» viene desactivada.
Activarla desde GUI crea un icono propio con menú Abrir Linux ScrollMaster / Salir de Linux ScrollMaster y
persiste solo `$XDG_CONFIG_HOME/linux-scrollmaster/desktop.json` (o `~/.config/linux-scrollmaster/desktop.json`).
Cerrar oculta la ventana únicamente si la bandeja existe y la preferencia está
activada. Salir termina el controlador y permite al guardián recuperar botones.
Al desactivar se muestra primero la ventana. Reiniciar Linux ScrollMaster recupera la preferencia;
la ventana se abre normalmente. La bandeja no habilita autostart ni instala servicios;
el [inicio automático voluntario](autostart.md) posterior tiene su propio interruptor.

Un panel sin soporte de bandeja puede no mostrar el icono aunque la biblioteca lo
cree: abrir Linux ScrollMaster de nuevo recupera la ventana y permite desactivar la opción.
Al iniciar sesión el autostart puede adelantarse al panel: en Omarchy la ventana se
abrió ~0,7 s antes de que quickshell registrara `org.kde.StatusNotifierWatcher`, y crear
el icono falló («failed to register to the StatusNotifierWatcher»). Antes ese error
dejaba la preferencia como inactiva y cerrar la ventana terminaba el proceso. Ahora
`lifecycle::initialize` conserva la preferencia leída aunque el icono falle, reintenta
crearlo cada 2 s durante 2 minutos en el hilo principal y, al cerrar, vuelve a
intentarlo antes de decidir. Solo se oculta la ventana si el icono quedó creado; sin
panel, cerrar sigue terminando el proceso para no dejarlo invisible. Verificado con un
bus D-Bus aislado (`dbus-run-session`), backend sin hardware y un watcher mínimo que
aparece 5 s después: el icono se registró, cerrar ocultó la ventana y el proceso siguió
vivo. Sin watcher, cerrar terminó el proceso como antes.

No se promete que el panel exista. Linux ofrece el menú, no eventos universales de
clic izquierdo. Dependencia de runtime añadida a Arch: libayatana-appindicator.
Se habilita la característica tray-icon de Tauri ya disponible en caché.

Se reutiliza la escritura JSON atómica 0600 para las preferencias; archivos
corruptos se conservan y se informa del error. Los perfiles no se modifican.

Seis pruebas nuevas verifican exclusión/activación/reinicio, socket obsoleto,
permisos/enlaces, valores por defecto, persistencia y conservación de archivos
corruptos. Suite: 151 pruebas (122 Python, 17 Vue, 12 Rust). Los sockets temporales
requieren ejecutar cargo test fuera del sandbox del agente, que bloquea AF_UNIX;
no requieren red remota, root ni hardware en una sesión Linux normal.

Compilación y pruebas aprobadas. Pendiente aceptación gráfica de bandeja y segundo
inicio en la sesión real; no se activó la preferencia del usuario durante estas pruebas.

Regresión de cierre: una repetición de la suite detectó exclusión transitoria
durante reinicio. Se libera el lock explícitamente al destruir el propietario
(incluidos descriptores duplicados) y se reintenta adquirirlo si el propietario
sale mientras se intenta activar su ventana. Regresiones verifican ambos casos,
además de mantener exclusión mientras sigue vivo.

La cabecera ofrece «Preferencias» para desplazar y enfocar directamente la sección
de bandeja; se añadió tras la dificultad del usuario para encontrarla al final.
La biblioteca Wayland se compila a un temporal y se publica con rename atómico:
compilar mientras un guardián la tiene mapeada no trunca su archivo activo.

El incremento de inicio automático usa una entrada XDG propia y abre la ventana
normalmente. No cambia estas reglas de cierre/bandeja, no habilita perfiles y no
añade un daemon. Suite vigente y aceptación pendiente en [autostart.md](autostart.md).
