# Atajos personalizados

## Alcance y uso

El incremento del hito C añade una combinación de tecla a los botones laterales y
a cualquiera de las cinco acciones del editor de gestos. En «Botones y gestos»,
elegir «Atajo personalizado…», marcar Ctrl, Alt, Shift o Super y elegir la tecla.
Los laterales tienen «Aplicar atajo»; los gestos conservan su botón de aplicación
conjunto. Editar el formulario y recibir sondeos no aplica el borrador. Guardar
un perfil después de aplicar conserva la asignación confirmada.

El selector ofrece letras A–Z, dígitos, F1–F12, flechas, navegación y teclas básicas
de edición. Permite una tecla sin modificadores. Envía una combinación completa,
una vez al pulsar el lateral o al soltar el gesto; mantener el botón no repite.
No captura el teclado global, no ejecuta comandos y no implementa macros.

La salida usa las posiciones de un teclado virtual US propio. No se reinterpretan
según la distribución física del usuario; no se ofrecen puntuación, teclas muertas,
AltGr, bloqueo de mayúsculas ni teclado numérico. El compositor puede reservar una
combinación; la aplicación activa decide su significado. No se promete X11 ni
compositores que no ofrezcan el protocolo de teclado virtual.

## Contrato y capas

- `bridge/actions.py` concentra teclas, etiquetas, modificadores y validación. El
  adaptador anuncia el catálogo solo para controles con consumidor residente y
  capacidad de captura. La GUI no acepta códigos numéricos de entrada.
- `ShortcutEditor.vue` edita una combinación; `ActionSelect.vue` comparte la
  selección entre laterales y gestos; `ButtonAssignment.vue` conserva el borrador
  y solicita aplicar. `src/shortcuts.ts` centraliza formato y presentación.
- El contrato interno es `custom:ctrl+shift:KeyS`, con modificadores opcionales
  en orden Ctrl/Alt/Shift/Super y tecla de catálogo cerrado. Se rechazan
  repeticiones, orden distinto, valores desconocidos y texto adicional en servicio,
  motor y guardián. No es una interfaz para comandos.
- Se conserva el consumidor HID++ único, el preflight de capacidades y la lectura
  de reporting sin caché. El guardián se prepara antes de desviar el botón y sigue
  restituyendo reporting al cerrar o perder el worker.
- C valida de nuevo tecla y máscara. Envía máscara, pulsación, liberación y
  restaura los modificadores que mantenía el propio teclado virtual. Si falla la
  confirmación no informa éxito; el cierre intenta liberar la tecla pendiente
  y deja máscara cero. No se garantiza entrega si se pierde el compositor.

## Persistencia

Schema 9 permite atajos personalizados en laterales y dentro de gestos.
`button_schema` calcula la versión mínima al guardar; se mantienen schemas 1–8
y sus objetos sin migración implícita. Restaurar, reconciliar y exportar/importar
preservan la cadena validada. Importar no activa perfiles. Las reglas por aplicación
siguen utilizando las asignaciones del perfil efectivo, sin otro motor de atajos.

No se modificó el perfil real del usuario, las preferencias de bandeja ni la
configuración de Solaar, Hyprland, Omarchy o servicios.

## Verificación y límites de evidencia

Incremento del 19 de octubre de 2026. **199 pruebas aprobadas**: 156 Python
(incluyen tres pruebas del C real con transporte Wayland simulado), 17 de estado
Vue, 5 de distancia, 4 de formato de atajos, 4 de componentes Vue y 13 Rust.
Son 21 regresiones nuevas sobre las 178 anteriores.

Las pruebas cubren validación, schema mínimo, pulsaciones sin repetición,
cancelación, guardián, catálogo según capacidades, lectura confirmada, devolución
nativa y guardar/restaurar/reconciliar/importar. La prueba C comprueba todas las
teclas 0–255 y máscaras 0–127, la secuencia de salida, restauración de la máscara
anterior y liberación pendiente tras fallo. Las pruebas de componentes compilan
los SFC reales y ejecutan sus eventos con el renderer Vue en memoria: borrador
sin escrituras, sondeos, Aplicar, cambios externos, estado ocupado y errores.

`pnpm test`, `pnpm build`, `cargo check --offline`, `cargo test --offline` y build
debug integrado aprobados. La suite regular necesita compilador C, pkg-config y
cabeceras Wayland/xkbcommon ya requeridos para compilar Linux ScrollMaster; no abre una pantalla,
no requiere mouse, red, root ni permisos de entrada.

**Pendiente aceptación física y gráfica:** aplicar una combinación en cada lateral
y un gesto, comprobar la aplicación receptora, soltar/cancelar, y guardar/restaurar/
reconectar un perfil schema 9. También siguen pendientes las aceptaciones de schema
8, bandeja, reglas por aplicación y diálogos registradas en `plan.md`. No se abrió
la GUI ni se cambiaron asignaciones reales durante este incremento.
