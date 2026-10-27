# Instancia única e inicio en el área de notificaciones

Contrato vigente del 27 de octubre de 2026: la preferencia de bandeja controla
tanto el inicio con ventana oculta como continuar después de cerrarla. No cambia
el inicio automático XDG, que conserva su propio interruptor.

Linux ScrollMaster conserva un único controlador por sesión mediante un bloqueo de archivo
mantenido durante toda la vida del host. Un segundo inicio envía únicamente `show`
al socket privado y termina antes de crear el worker o una segunda GUI. Si el
propietario no responde no se abre otro controlador. El bloqueo se libera por el
kernel incluso tras un fallo; un nuevo propietario retira el socket obsoleto.

El socket está bajo `$XDG_RUNTIME_DIR/linux-scrollmaster/`, directorio 0700 cuyo propietario se
verifica. No se aceptan directorios públicos ni enlaces simbólicos. Las lecturas
IPC se acotan a cuatro bytes y 200 ms; no existe ejecución de comandos. Requiere
Rust 1.89+ para el bloqueo estándar `File::try_lock`, sin otra dependencia de bloqueo.

La preferencia «Iniciar en el área de notificaciones y continuar al cerrar» viene desactivada.
Activarla desde GUI crea un icono propio con menú Abrir Linux ScrollMaster / Salir de Linux ScrollMaster y
persiste solo `$XDG_CONFIG_HOME/linux-scrollmaster/desktop.json` (o `~/.config/linux-scrollmaster/desktop.json`).
Cerrar oculta la ventana únicamente si la bandeja existe y la preferencia está
activada. Salir termina el controlador y permite al guardián recuperar botones.
Al desactivar se muestra primero la ventana. Reiniciar Linux ScrollMaster recupera la preferencia;
si está activada, empieza con la ventana oculta y el icono en el panel. Sin ella,
se muestra la ventana. Activarla no oculta la ventana que se está usando; afecta
al próximo inicio y al cierre. La bandeja no habilita autostart ni instala servicios;
el [inicio automático voluntario](autostart.md) posterior tiene su propio interruptor.

La ventana Tauri se crea con `visible: false` para evitar que aparezca brevemente
antes de leer la preferencia. Un archivo corrupto o incompatible se conserva y
se muestra la ventana con el error. El backend y el sondeo residente se inician
igual aunque la ventana esté oculta, sin depender del renderizado de WebKit.

Un panel sin soporte de bandeja puede no mostrar el icono aunque la biblioteca lo
cree: abrir Linux ScrollMaster de nuevo recupera la ventana y permite desactivar la opción.
Al iniciar sesión el autostart puede adelantarse al panel: en Omarchy la ventana se
abrió ~0,7 s antes de que quickshell registrara `org.kde.StatusNotifierWatcher`, y crear
el icono falló («failed to register to the StatusNotifierWatcher»). Antes ese error
dejaba la preferencia como inactiva y cerrar la ventana terminaba el proceso. Ahora
`lifecycle::initialize` conserva la preferencia leída aunque el icono falle, reintenta
crearlo cada 2 s durante 2 minutos en el hilo principal y, al cerrar, vuelve a
intentarlo antes de decidir. El arranque permanece oculto hasta crear el icono;
tras cinco reintentos fallidos (unos 10 s, más el tiempo de registro) muestra la
ventana. Los reintentos restantes nunca vuelven a ocultarla. Abrir desde el menú,
el icono o un segundo inicio cancela también esa espera. Solo se oculta al cerrar
si el icono quedó creado; sin panel, cerrar termina el proceso.

La corrección anterior de reintentos fue verificada con un
bus D-Bus aislado (`dbus-run-session`), backend sin hardware y un watcher mínimo que
aparece 5 s después: el icono se registró, cerrar ocultó la ventana y el proceso siguió
vivo. Sin watcher, cerrar terminó el proceso como antes.

No se promete que el panel exista. El backend actual `ksni` de `tray-icon` publica
StatusNotifierItem por D-Bus y permite abrir con un clic izquierdo, además del menú.
No se necesita detectar un doble clic: quickshell puede absorber el segundo.

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

El inicio automático usa una entrada XDG propia y respeta esta preferencia: comienza
en el área de notificaciones si está activada y muestra la ventana si está desactivada.
No añade un daemon. Contrato en [autostart.md](autostart.md).

Verificación del 27 de octubre de 2026: `pnpm test` (252 casos), `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml` y
`cargo test --offline --manifest-path src-tauri/Cargo.toml` (594 casos) aprobados;
846 pruebas en total. Las suites de sockets requieren salir del sandbox del
agente; dentro de él fallan por restricciones de IPC local.

Cuatro regresiones Rust cubren configuración inicial oculta, icono disponible,
panel tardío, ausencia de panel, opción desactivada y apertura explícita durante
la espera. No requieren compositor ni mouse. Pendiente: aceptación gráfica del
nuevo arranque en el panel real y del próximo inicio de sesión XDG. Las pruebas
de política no acreditan visibilidad real del icono ni aceptación física.
