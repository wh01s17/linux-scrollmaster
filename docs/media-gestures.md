# Controles multimedia en gestos

Por petición del usuario, el selector de cada gesto añade Reproducir/Pausar,
Pista anterior, Pista siguiente, Subir volumen, Bajar volumen y Silenciar/Activar
sonido. Se aplican mediante el mismo botón del pulgar y el motor direccional ya
verificado para Copiar/Pegar. No se alteran asignaciones ni perfiles existentes.

El catálogo vive en `bridge/actions.py`. La GUI recibe sus opciones del adaptador,
sin duplicar etiquetas ni ofrecer acciones inexistentes. Los laterales mantienen
su catálogo anterior: este incremento añade multimedia a los gestos.

La salida usa teclas multimedia Linux a través del teclado virtual Wayland:
KEY_PLAYPAUSE 164, PREVIOUSSONG 165, NEXTSONG 163, VOLUMEUP 115, VOLUMEDOWN 114,
MUTE 113. Cada acción envía pulsación y liberación, con máscara de modificadores 0,
y restaura la máscara anterior. Edición sigue enviando Ctrl+tecla. La frontera C
en este incremento solo admitía esas seis teclas con máscara 0 y las cuatro teclas
de edición con Ctrl; el catálogo se amplía con [atajos personalizados](custom-shortcuts.md),
sin comandos ni códigos arbitrarios. El símbolo de ABI cambia a
`scrollmaster_keyboard_action` para evitar mezclar firmas de bibliotecas antiguas.

El compositor decide cómo gestionar teclas multimedia y qué reproductor recibe
pista/reproducción. No se modifica la configuración de Hyprland ni se ejecutan
comandos externos; no se promete seleccionar un reproductor concreto.

Schema 7 permite acciones multimedia en gestos; versiones anteriores siguen
siendo válidas. Guardar, restaurar, reconectar e importar/exportar preservan el
catálogo multimedia. Documentos anteriores con acciones desconocidas se rechazan
sin sobrescribirlos.

Cinco pruebas nuevas verifican salida sin Ctrl, error de salida, ejecución única,
comandos del guardián, schema y ciclo de perfiles con dobles. Suite: 156 pruebas
(127 Python, 17 Vue, 12 Rust), frontend y binario integrado compilados.
Aceptación física de multimedia pendiente; no confundir con la aceptación previa
de Copiar/Pegar mediante gestos izquierda/derecha.

Aceptación posterior: usuario confirmó funcionamiento de multimedia en la prueba
volumen/reproducción, pero señaló precisión direccional insuficiente. No se da por
validada la paridad con Windows ni todas las acciones de pista/silencio. Corrección
y evidencia en directional-gestures.md.
