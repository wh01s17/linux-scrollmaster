# Gestión e intercambio de perfiles

Implementados renombrado, eliminación, exportación e importación con vista previa.
La gestión local funciona con el mouse desconectado e incluso si Solaar no está
disponible. No configura el hardware por gestionar un archivo.

`bridge/profiles.py` contiene las transformaciones puras; el servicio conserva
persistencia atómica y bloqueo. Renombrar actualiza referencias activas; eliminar
desarma ese perfil sin cambiar ajustes actuales. Importar asocia al dispositivo
seleccionado, conserva perfiles existentes, renombra duplicados y nunca importa
activaciones. Se admite hasta 100 perfiles y 128 KiB; versiones desconocidas o
datos inválidos fallan antes de escribir.

`ProfileManager.vue` separa el gestor de la pantalla de hardware. El usuario ve
los nombres que se importarán antes de confirmar y una confirmación independiente
para eliminar. Los errores locales no descartan una lectura válida del mouse.

La frontera Rust `profile_files.rs` abre diálogos GTK nativos. Solo accede al
archivo elegido por el usuario; no expone un comando para rutas arbitrarias.
El diálogo queda fuera del mutex/worker HID++; los botones siguen consumiéndose.
Exportar usa temporal 0600, fsync y reemplazo; importar limita tamaño y JSON antes
de la validación del servicio. GTK ya era dependencia de Tauri y se referencia
directamente sin incorporar un framework nuevo. IPC acotado a 256 KiB para perfiles.

116 pruebas aprobadas: 93 Python, 17 estado Vue y 6 Rust. Hay casos de gestión
offline, roundtrip de perfiles con acciones, límite de tamaño/cantidad, fallos de
disco, conflictos de nombres, referencia activa y lectura/escritura de exportación.
La compilación de frontend y host pasa. Los diálogos y clicks nuevos todavía
requieren aceptación gráfica; no se presentan como prueba con usuario completada.

N5 ya incorpora un [núcleo nativo de perfiles](native-profiles.md) con validación
1–10, operaciones locales y persistencia en Rust. Sus pruebas usan JSON compartido
y archivos temporales; todavía no sustituye el servicio Python de esta interfaz.
