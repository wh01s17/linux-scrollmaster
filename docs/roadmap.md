# Hoja de ruta

## Entregado: corte vertical 0.1

- [x] Tauri 2 + Vue 3 + TypeScript + Rust + backend Solaar aislado.
- [x] Detección Bolt, batería, seis ajustes reales con lectura de confirmación.
- [x] Controles visuales, temas, perfiles locales y restauración.
- [x] Reconciliación voluntaria mientras la GUI está abierta, validación y conflictos.
- [x] Pruebas automáticas y prueba física de escritura/restauración.

## Prioridad inmediata

1. **Independencia de Solaar obligatoria:** Hito N, transporte e implementación
   HID++ propios en Rust, después ajustes, eventos, recuperación y servicio nativo.
   Cierre: ejecución y paquete sin Solaar/Python, perfiles conservados y aceptación
   del hardware. N1 completado sin hardware con 22 pruebas; siguiente incremento:
   N2 aceptado por Bolt. N3 añade seis setters y remapeo secundario; roundtrips
   DPI y Atrás aceptados. N4 inicia recepción pasiva con escucha en reposo
   aceptada y preserva informes intercalados durante consultas/confirmaciones.
   El transporte admite consumidor síncrono y propietario pasivo con consultas
   repetidas/cierre verificados en reposo. Implementación N1–N6 completa: GUI con
   backend nativo optativo, captura con transiciones y paquete sin Solaar/Python.
   Falta la [aceptación física](native-acceptance.md) antes de cambiar el default.
   Ver [plan](native-backend.md) e [integración](native-gui.md).
2. Hito de fiabilidad aceptado con el backend actual: ampliar cobertura física a
   Bluetooth, extracción del receptor y múltiples receptores cuando estén disponibles.
3. Accesibilidad con lector de pantalla, navegación por teclado y escalas de pantalla.
4. Aceptar inicio automático XDG voluntario ya implementado. Servicio de sesión
   independiente de la GUI y eventos udev/hotplug siguen pendientes.
5. Alt/Ctrl, edición y Copiar/Pegar por gestos izquierda/derecha verificados con el usuario.
   Completar multimedia, otros sentidos, perfiles schema 3–7 y desconexión con tecla mantenida.
6. Completar aceptación de reglas por aplicación, gestor de perfiles y bandeja; están
   implementados y probados automáticamente. Foco/lista Hyprland verificados realmente.

Solaar 1.1.20 sigue siendo necesario en la versión actual. Se mantiene durante la
transición; ampliar su compatibilidad no sustituye el Hito N. No ampliar funciones
antes de avanzar en la independencia, salvo correcciones de fallos del producto actual.

## Siguiente producto

Implementados: reglas por aplicación, editor de gestos conectado, multimedia, atajos propios,
cuatro acciones de espacios/ventanas Hyprland,
inicio automático XDG voluntario,
importación/exportación, renombrado/eliminación, instancia única y bandeja optativa.
La aceptación real pendiente se detalla en plan.md; no se atribuye a pruebas con dobles.
Pendientes de implementar: otras acciones del sistema/compositores, servicio independiente
y distribución Arch instalada y comprobada. El PKGBUILD sigue siendo preparación.
Los atajos propios tienen pruebas automáticas de motor, salida C, persistencia y
componentes; falta aceptación física. Véase [custom-shortcuts.md](custom-shortcuts.md).
Las cuatro acciones Hyprland también están probadas automáticamente y pendientes
de aceptación física: [desktop-actions.md](desktop-actions.md).
El inicio automático se implementó sin habilitarlo; aceptación del escritorio y
del próximo inicio de sesión pendientes: [autostart.md](autostart.md).

## Investigación posterior

Macros con cancelación; otros dispositivos por catálogo de capacidades; alternativas
locales a Smart Actions. Flow requiere estudio de red, portapapeles, seguridad y
consentimiento entre equipos: no se incluye en el alcance prometido del MVP.
