# Perfiles por aplicación — Hyprland

Implementación optativa con reglas locales schema 6. Se elige una aplicación de
las ventanas abiertas y un perfil guardado; no se escriben identificadores a mano.
La GUI muestra aplicación actual, perfil efectivo y reglas editables.

## Precedencia

1. Sin perfil general marcado para reaplicación no se ejecutan reglas.
2. Con perfil general armado y coincidencia exacta de `class`, se usa el perfil de
   esa aplicación; al cambiar a otra ventana o perder foco se vuelve al general.
3. Si Hyprland no responde se usa el general y se explica el fallo. Las reglas se
   conservan. Fuera de Hyprland la configuración manual sigue disponible.
4. Editar un ajuste/botón o restaurar un perfil desarma el general antes de escribir
   hardware. Así las reglas tampoco revierten la intención manual.

La reconciliación existente ejecuta el perfil efectivo; cada ajuste se confirma
por lectura real. La regla nunca habilita automatismos por sí misma. Cambiar el
foco no altera la selección general persistida. Renombrar un perfil actualiza sus
reglas; eliminarlo retira referencias. Importar/exportar transporta contenidos de
perfiles, nunca instala reglas ni activa perfiles externos.

## Integración y límites

`bridge/desktop.py` abre un socket Unix por consulta, desde XDG_RUNTIME_DIR y la
firma de sesión actual. El adaptador de foco solo usa `j/activewindow` y `j/clients`;
las [acciones de escritorio](desktop-actions.md) posteriores añaden versión,
comprobación y cuatro dispatchers fijos separados de estas consultas. No hay shell
ni cambios de configuración. Deadline total por intercambio de 1 s y respuesta
máxima 1 MiB. Solo devuelve clases; descarta títulos, PID y nombres de documentos.
El perfil admite hasta 100 reglas, sin expresiones regulares ni comandos.

Fuente primaria: [IPC de Hyprland](https://wiki.hypr.land/IPC/). La ruta del socket
y las dos consultas se contrastaron además con la sesión real del usuario.

Se verifica el foco en cada sondeo nativo (5 s después de completar la consulta
anterior); no se promete reacción instantánea ni cambios transaccionales de todo
el mouse. El foco puede cambiar mientras se realizan escrituras HID++; el siguiente
sondeo reconcilia. Todavía no se consume socket2 para disparar cambios inmediatos.
No hay adaptación genérica de foco para GNOME/KDE/X11.

## Evidencia

13 pruebas nuevas sin compositor/hardware: protocolo, respuesta acotada, timeout,
foco ausente, regreso al general, reglas desarmadas, intención manual, persistencia,
referencias de renombrado/eliminación e importación sin automatismos. Suite actual:
145 pruebas (122 Python, 17 estado Vue, 6 Rust).

Lectura **real** del adaptador: Hyprland disponible, cinco aplicaciones detectadas.
No se persistieron reglas ni se modificó el mouse para esa comprobación. Pendiente
la aceptación GUI de cambios efectivos al alternar aplicaciones con hardware real.

## Migración nativa N5

El [worker Rust](native-worker.md) ya tiene [foco y catálogo propios](native-focus.md),
separados de los dispatchers. Reutiliza el transporte Unix validado, conserva la
precedencia general/regla/manual y devuelve solo clases. El presupuesto conjunto
de lectura activa/catálogo es 1 s, y los errores no reutilizan el foco anterior.
Las reglas se probaron con dispositivo/foco sintéticos y el catálogo también con
el proceso Rust real y un compositor simulado. Esta evidencia no acredita cambios
de perfil nativos en hardware; la GUI aún conserva el adaptador Python.
