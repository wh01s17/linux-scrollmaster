# Gestos direccionales — incremento implementado

El botón del pulgar admite una configuración explícita de cuatro direcciones y
pulsación sin desplazamiento. Las acciones disponibles son Copiar, Pegar, Cortar,
Deshacer, multimedia, [atajos personalizados](custom-shortcuts.md) o ninguna.
No se activa ni se guarda automáticamente. El editor mantiene
la identidad visual de Linux ScrollMaster y permite devolver la función original.

Estado vigente: cuatro direcciones probadas con usuario (edición izquierda/derecha,
volumen arriba/abajo), precisión corregida aceptada, raw XY/restauración real
confirmados. La distancia estable opcional por DPI se incorporó después:
[contrato schema 8 y evidencia](gesture-distance.md). Sin promesa de equivalencia
con Windows; los umbrales anteriores conservan su comportamiento.
Las secciones posteriores conservan la evolución y evidencia de cada incremento.

## Contrato y recuperación

`buttons['195']` puede contener `{type:'gestures', threshold, actions}` en schema 5.
Schemas 1–4 siguen siendo válidos. Guardar/restaurar, reconciliar e importar/exportar
preservan el objeto completo. Se valida antes de escribir cualquier ajuste escalar.
Los otros botones solo admiten las acciones que ya ofrecían.

El adaptador 1.1.20 exige capacidades DIVERTABLE y RAW_XY, verifica que no exista
intercepción ajena y prepara el consumidor Wayland antes de escribir. Registra
CID 195 en el guardián, activa diversion temporal y raw XY y confirma flags 0x11
mediante lectura directa. Las notificaciones 1B04 de dirección 0 entregan el conjunto
de botones; dirección 0x10 aporta dos enteros signed big-endian de 16 bits.
Se usa el mismo handle y consumidor que las peticiones HID++; no hay lector paralelo.

El motor independiente acumula desplazamiento mientras se mantiene el botón.
El umbral se expresa en cuentas crudas del sensor (20–2000; depende de DPI), no en
píxeles. Decide al soltar; exige que un eje domine al otro por un factor 1,5.
Descarta diagonales ambiguas, recorridos que vuelven al origen después de superar
el umbral, gestos de más de 10 s, cambios de asignación mientras se mantiene,
notificaciones truncadas y cancelación por desconexión/suspensión. La salida se
consume antes de enviarla para que un fallo no repita la acción.

Al desactivar, se cancelan gestos pendientes y se comprueba la liberación de ambos
flags. Ante EOF/crash del worker, el guardián limpia ambos usando bits valid 0x22,
sin cambiar el mapeo nativo previo. Conserva los límites de recuperación documentados
en held-modifiers.md: hardware desconectado o fallo del propio guardián no permiten
prometer una restitución absoluta. El puntero queda capturado durante el gesto.

## Evidencia y pendiente

Fuente primaria inspeccionada: Solaar 1.1.20 instalado, hidpp20.py:
set_rawXY_reporting/_setCidReporting; settings.py: RawXYProcessing notification_handler.
No se reanalizó el instalador de Windows ni se copiaron recursos propietarios.

16 pruebas nuevas cubren direcciones, umbral, acumulación, cancelaciones, timeout,
flancos, coexistencia con Alt, schemas, readback, recuperación del guardián y el
ciclo de perfil/importación con dobles. La suite total tiene 132 pruebas:
109 Python, 17 estado Vue, 6 Rust. Frontend y host compilan offline.

**Verificación real de captura y recuperación:** `python tests/hardware_modifiers_roundtrip.py --gestures`
confirmó flags raw XY + diversion y retorno al reporting original al cerrar y tras
SIGKILL del worker. Los perfiles quedaron intactos; no se generaron teclas durante
la prueba. **Pendiente:** sentidos, umbral y acciones con el usuario desde GUI. La aceptación física previa de Alt/Ctrl
y edición no se atribuye automáticamente a los gestos. La interfaz lo marca como
experimental hasta completar esa aceptación. No se modificó la configuración del
usuario ni se activaron gestos durante las pruebas automatizadas.

Aceptación posterior: el usuario confirmó funcionamiento de Copiar/Pegar con
gestos izquierda/derecha configurados desde GUI. Solicitó añadir multimedia al
catálogo. No atribuir esta aceptación a multimedia ni a todos los sentidos.

## Ajuste de precisión tras aceptación multimedia

El usuario confirmó volumen/reproducción, pero informó direcciones confundidas o
no reconocidas.

Hallazgo Linux concreto: Solaar omite el primer raw XY en 1B04 versión >=5 por una
particularidad del MX Master 3S. El dispositivo real anuncia **versión 5**. El
adaptador ahora omite ese informe en cada pulsación solo cuando aplica la versión;
si no puede verificarla, bloquea la activación. Versiones anteriores mantienen sus
primeros informes. No se descarta una muestra por tamaño arbitrario.

La clasificación propia se actualiza: al superar el umbral con un eje 1,2 veces
mayor que el otro, fija la dirección hasta soltar. Un retorno o corrección al final
no la convierte en otra dirección ni en clic. Diagonales que nunca definen un eje
siguen descartándose. Esto sustituye la decisión final con factor 1,5 descrita
arriba; no es una atribución a Logitech. La GUI muestra la última dirección
reconocida (con el ritmo de sondeo, no como stream en tiempo real).

Cancelar mientras se mantiene el botón suprime nuevas acciones hasta observar su
liberación real; eventos de otros botones no rearman el gesto. Cinco regresiones
nuevas cubren informe inicial anómalo repetido, versión desconocida, correcciones,
diagonales y cancelación mantenida. Total actual: 161 pruebas (132 Python, 17 Vue,
12 Rust). Falta aceptación física de esta mejora; no prometer precisión equivalente
a Windows. Umbral todavía dependiente de DPI.

Aceptación posterior de la corrección: el usuario confirmó «ahora funciona mucho
mejor» al repetir arriba/abajo con volumen. Captura/restitución real también volvió
a pasar. Se acepta la mejora, sin afirmar equivalencia con Windows ni validar
todas las combinaciones posibles.
