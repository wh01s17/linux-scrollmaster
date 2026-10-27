# Linux ScrollMaster: plan de implementación y continuidad

**Actualización del 27 de octubre de 2026:** la GUI guarda cada cambio confirmado
en **Perfil actual**. **Guardar como…** crea copias con nombre sin consultar HID;
atajos y gestos ya no exigen aplicar manualmente. La carga al iniciar y al
reconectar es automática, sin casilla optativa. El sondeo nativo evita repetir
preflight/apply cuando la lectura fresca coincide. Contrato, fallos, pruebas y
aceptación física pendiente: [guardado automático](docs/automatic-profiles.md).
Las descripciones históricas siguientes de guardado manual y reaplicación
optativa quedan sustituidas por ese contrato. El paquete 0.2.1 usa Rust nativo;
el backend Python sigue disponible como ruta de desarrollo heredada (README).

**Corrección del 26 de octubre de 2026:** el sondeo cancelaba gestos físicos
incluso con DPI idénticos y descartaba nuevas pulsaciones durante la consulta.
Se conserva el recorrido provisional y se confirma su salida tras validar el
snapshot con los mismos DPI. Nueve regresiones nuevas; suites completas aprobadas
(819 pruebas). El usuario confirmó que funciona bien tras la corrección. Diagnóstico, contrato
y límites en [docs/gesture-polling.md](docs/gesture-polling.md).

Última actualización: 21 de octubre de 2026, America/Santiago. La evidencia física
conserva sus fechas originales. N1 aporta codec/transporte Rust, N2 ya está
aceptado por Bolt y N3 añade seis setters y remapeo secundario, con aceptación ampliada
pendiente. N4 añade recepción pasiva, restauración y guardián independiente con pruebas
sintéticas; el motor de acciones/gestos tiene entrega directa interna al consumidor.
Asignaciones y captura finita optativa ya implementadas con confirmación y pruebas
sintéticas. La salida nativa de escritorio ya tiene catálogo cerrado y preflight;
aceptación física y servicio integrado pendientes.
N5 ya tiene núcleo de perfiles/gestión local y persistencia Rust, con pruebas
en temporales y contrato compartido. Servicio síncrono y reconciliación de ajustes/
remapeos nativos ya conectados al backend Rust con protocolo simulado. El dueño
nativo ya admite arranque offline, lease y reconexión por identidad; integración
de acciones duraderas del guardián y sustitución del worker Tauri pendientes. Worker Rust JSON-lines
experimental ya implementado en proceso separado, con supervisor compartido.
Foco/catálogo Rust ya seleccionan reglas por aplicación con consultas acotadas,
pruebas de sockets y dispositivo simulados; aceptación física nativa pendiente.
No se sustituye el backend Solaar de la aplicación.
Los botones de perfiles ya se compilan para el guardián finito: solo se capturan
asignaciones de software; los remapeos nativos deben coincidir antes de activar.
Integración duradera de acciones con servicio/perfiles aún pendiente; evidencia sintética en
[botones de perfiles y guardián](docs/native-profile-actions.md).
El [propietario independiente de perfiles](docs/native-profile-guardian.md) ya se
conecta al servicio mediante `--serve-guarded`, con sesión/lease en el hijo y
cierre confirmado; no habilita todavía acciones de software. El [consumidor
pasivo duradero](docs/native-profile-events.md) ya atiende consultas y reposo,
con historial acotado y cancelación. Motor/salida y restauración siguen pendientes.

Este archivo es el punto de entrada para cualquier agente que retome el proyecto.
Describe el estado comprobado, las decisiones que deben conservarse y el trabajo
pendiente. No es una lista de funciones supuestas ni un reemplazo de la evidencia.
Actualizar este archivo al cerrar cada incremento y enlazar el informe detallado.

Corrección de los abortos del harness de guardianes del 22 de octubre: [limpieza tras fallos de prueba](docs/crash-guardian-cleanup.md).

## 1. Objetivo y alcance

Construir una aplicación Linux de código abierto para configurar el Logitech MX
Master 3S con una experiencia accesible, comparable en facilidad a Options+.
Las tareas habituales deben hacerse en la GUI sin escribir comandos, códigos HID
o archivos de configuración. El objetivo exige comunicación real con el mouse.

- Nombre e identidad actuales: **Linux ScrollMaster**; conservar diseño, ilustración y temas.
- Entorno principal: Arch Linux, Omarchy, Hyprland y Wayland.
- Stack implementado: Vue 3 Composition API, TypeScript, Vite, Tauri 2, Rust y
  proceso Python aislado que utiliza Solaar instalado por el sistema.
- Arquitectura objetivo obligatoria: backend hidraw/HID++ propio en Rust;
  ejecución y paquete final sin Solaar ni Python. Prioridad: Hito N.
- Licencia del producto: GPL-3.0-or-later.
- Versión actual: 0.1.0, MVP de desarrollo, sin publicación de producción.

La fiabilidad inicial está aceptada para Bolt. La dirección vigente del usuario
exige independencia de Solaar; ya no se trata como mejora opcional.
Los siguientes hitos son trabajo pendiente, no funcionalidades entregadas ni autorización
para instalar servicios, conceder permisos o modificar el escritorio.

## 2. Lectura inicial y reglas para el siguiente agente

1. Leer `AGENTS.md`, este archivo, `README.md` y `docs/architecture.md`.
2. Consultar `git status --short` antes de editar. El código inicial ya quedó
   versionado en commits separados de backend, escritorio, empaquetado y documentación.
   Antes de esa entrega no había commits; esa observación histórica ya no describe
   el estado actual. No descartar cambios del usuario ni ejecutar reset/clean.
3. Usar el MCP codebase-memory para descubrir código. Confirmar proyecto y frescura
   con `list_projects`/`index_status`, localizar símbolos, trazar llamadas y consultar
   snippets; comprobar cobertura de las rutas usadas y leer fuente ante huecos.
   El índice es una ayuda; el estado del disco y las pruebas determinan el resultado.
4. No analizar software propietario. El comportamiento se diseña a partir del
   protocolo HID++ (documentado por Solaar y logiops) y de pruebas propias.
5. Hacer cambios pequeños; ejecutar las pruebas pertinentes después de cada cambio.
   No delegar a otros agentes sin petición o instrucción aplicable.
   Mantener cada archivo de código en un máximo de 1000 líneas; dividir por
   responsabilidad y justificar por escrito cualquier excepción concreta.
6. Verificar procesos antes de abrir otra instancia: Linux ScrollMaster pudo quedar abierta al
   terminar la sesión. No asumir que sigue ejecutándose ni iniciar duplicados.

### Restricciones que deben conservarse

- Nunca ejecutar el instalador de Logitech ni binarios propietarios extraídos.
- No copiar/redistribuir código, imágenes, logotipos o tipografías propietarias.
- No ejecutar la GUI como root ni conceder permisos globales de entrada.
- No editar configuraciones de Solaar, Hyprland, Omarchy o servicios del usuario.
- No realizar descargas grandes ni instalaciones privilegiadas sin aprobación.
- No detener Solaar/logid automáticamente. Explicar el conflicto y dejar que el
  usuario cierre su controlador habitual.
- Pedir participación del usuario para desconectar físicamente, suspender o
  comprobar tacto/comportamiento. Las pruebas físicas no se sustituyen con mocks.
- No habilitar diversion de botones sin consumidor real, cancelación y recuperación.
- Mantener las preferencias del usuario; no restaurar un baseline histórico por
  considerarlo arbitrariamente el valor correcto.
- El diagnóstico general de fallos de suspensión de Linux **no pertenece al
  alcance de Linux ScrollMaster**, por indicación expresa del usuario. No reabrirlo como bloqueo,
  responsabilidad o siguiente tarea. La recuperación con Linux ScrollMaster fue verificada.

## 3. Estado ejecutivo comprobado

**Hito de fiabilidad: completado en el MX Master 3S por Logi Bolt probado.**
Se verificó el ciclo abrir → cambiar ajustes → guardar perfil → restaurar →
apagar/encender mouse → recuperar automáticamente los valores guardados.
También se comprobó apertura tras reinicio y reanudación con Linux ScrollMaster abierta.

Hay **654 pruebas automatizadas aprobadas**: 178 Python (incluyen diez del C real
con transporte simulado), 17 del estado Vue, 5 de distancia, 4 de formato de atajos,
8 de componentes Vue, 7 de estado de preferencias y 435 Rust (360 biblioteca,
20 host, 44 integración y once CLI; N1–N5 experimentales incluidos).
Frontend, host y ejecutable integrado compilan. Estos resultados no equivalen
a cobertura física de Bluetooth, todos los escritorios o todos los fallos posibles.

No usar porcentajes como criterio de aceptación: la estimación conversacional
de 30–40 % del producto completo era orientativa, sin cálculo de esfuerzo.
Usar las listas y criterios de este plan para medir avance.

### Matriz de producto

| Funcionalidad | Estado actual | Evidencia o límite |
|---|---|---|
| Detección MX Master 3S | Implementada y verificada por Bolt | Identidad estable, modelo y estado reales |
| Conexión Bolt | Verificada | Receptor C548, mouse B034, HID++ 4.5 |
| Conexión Bluetooth | Ruta implementada, prueba física pendiente | No cambiar emparejamiento sin participación del usuario |
| Batería | Lectura real verificada | 40 % en la sesión; si falta se muestra «—» |
| DPI | Implementado y verificado desde GUI | Rango anunciado 200–8000, paso 50; slider y entrada exacta |
| SmartShift | Implementado y verificado | Rango Solaar 1–50; ajustar en modo dentado |
| MagSpeed | Modo dentado/libre, inversión y resolución implementados | Lecturas reales y prueba táctil de giro libre |
| Rueda horizontal | Inversión implementada y verificada | Velocidad y reasignaciones alternativas pendientes |
| Guardar/restaurar perfiles locales | Implementado y verificado desde GUI | Persistencia propia, seis valores, lectura posterior |
| Reaplicación al reconectar | Implementada y verificada por Bolt | Voluntaria, con Linux ScrollMaster abierta; perfil armado offline probado |
| Apertura tras reinicio | Verificada | Recuperó valores diferentes de los del perfil persistido |
| Suspensión/reanudación | Verificada con Linux ScrollMaster abierta | Lectura independiente tras volver; no prueba necesidad de reescritura |
| Botones laterales y botón de gestos | Funciones nativas y Alt/Ctrl mantenidos implementados; Alt/Ctrl verificados físicamente en laterales | GUI, consumidor HID++, guardián y teclado Wayland; gestos izquierda/derecha verificados |
| Acciones Copiar/Pegar/Cortar/Deshacer | Implementadas y verificadas con el usuario en los laterales | Las cuatro acciones verificadas con usuario, pulsación/liberación y perfiles schema 4 |
| Gestos direccionales | Implementados y verificados con usuario en izquierda/derecha | Raw XY y recuperación real; Copiar/Pegar desde GUI |
| Gestos multimedia | Implementados y probados automáticamente | Volumen/reproducción confirmados; precisión corregida aceptada; seis opciones schema 7 |
| Distancia de gesto independiente de DPI | Implementada y probada automáticamente | Captura/DPI/restauración reales verificados; comparación física del recorrido pendiente; schema 8 |
| Editor de atajos personalizados | Implementado y probado automáticamente | Laterales y gestos; Ctrl/Alt/Shift/Super y selección de tecla, schema 9; aceptación física pendiente |
| Acciones de espacios/ventanas | Implementadas y probadas automáticamente | Cuatro acciones Hyprland 0.55/0.56 Lua, schema 10; capacidades reales leídas, aceptación física pendiente |
| Perfiles por aplicación | Implementados para Hyprland, probados automáticamente | Foco/lista reales leídos; cambio efectivo GUI/hardware pendiente |
| Instancia única y bandeja opcional | Implementadas y probadas automáticamente | Aceptación gráfica pendiente; desactivada por defecto |
| Inicio automático XDG | Implementado y probado en temporales, desactivado por defecto | Opción explícita en Preferencias; requiere escritorio que procese XDG; aceptación de inicio de sesión pendiente |
| Servicio de sesión independiente | Pendiente | La bandeja mantiene el proceso actual; sin daemon instalado |
| Importar/exportar/renombrar/eliminar perfiles | Implementados y probados automáticamente | Diálogos GTK nativos; aceptación gráfica pendiente; sin aplicar importaciones |
| Paquete Arch | Receta preparada | Sin instalación ni validación de release/chroot |
| Backend HID++ propio sin Solaar | N1/N2 aceptados; seis setters y remapeo N3 implementados; captura optativa N4 probada sintéticamente; N5–N6 pendientes | Lecturas Bolt y roundtrip DPI nativo verificados; aceptación ampliada N3 y captura/acciones nativas físicas pendientes. La GUI aún usa Solaar/Python |
| Macros, otros modelos, Smart Actions y Flow | Investigación futura | Sin promesa de compatibilidad propietaria |

## 4. Trabajo realizado y decisiones que no deben perderse

### 4.1 Investigación inicial — completada para el MVP

- Se contrastó con Solaar, logiops y subsistemas Linux. DPI, ruedas y SmartShift
  son ajustes HID++; perfiles, atajos, gestos y foco requieren software anfitrión.

### 4.2 Backend híbrido — implementación actual de transición

Rust supervisa un proceso Python residente. Solo `bridge/solaar_adapter.py` importa
interfaces internas de Solaar. Se aprovecha una implementación HID++ funcional
sin introducir todavía un transporte Rust propio ni un segundo driver concurrente.
El adaptador tiene contrato v1 y admite únicamente **Solaar 1.1.20**, versión probada.
Versiones distintas fallan con mensaje explícito antes de enumerar/escribir.
La versión se obtiene con `importlib.metadata`, porque `solaar.__version__` puede
devolver el hash Git de Linux ScrollMaster. Corrección y evidencia: `docs/native-buttons.md`.

No es una API pública estable de Solaar. Mientras exista este backend, no eliminar
el control de versión para silenciar errores. La dirección vigente exige sustituirlo
por transporte e implementación HID++ propios en Rust, conservando el contrato,
los perfiles y las garantías de recuperación. No ampliar versiones de Solaar como
sustituto de esa migración. Ver ADR-002 y [backend nativo](docs/native-backend.md).

### 4.3 Controles y validación real

Se permiten seis settings:

| Clave interna | Uso en GUI | Regla importante |
|---|---|---|
| `dpi` | Sensibilidad | Opciones anunciadas por el dispositivo, no rango supuesto |
| `smart-shift` | Umbral SmartShift | 1–50; 50 equivale al extremo siempre dentado |
| `scroll-ratchet` | Modo de rueda | 1 giro libre; 2 dentado/SmartShift |
| `hires-smooth-invert` | Inversión vertical | Booleano estricto |
| `hires-smooth-resolution` | Alta resolución | Booleano estricto; efecto percibido depende de la aplicación |
| `thumb-scroll-invert` | Inversión lateral | Booleano estricto |

El adaptador valida el lote antes de escribir, comprueba lecturas previas y usa
`write(save=False)` más lectura `cached=False` para confirmar. Se conservan
capacidades y handles, **no valores de ajustes en caché**.

SmartShift devuelve el marcador 1 en giro libre y oculta el umbral. Restaurar modo
antes del umbral fue necesario y está corregido. No tratar ese 1 como prueba de
que el usuario eligió umbral mínimo. Las escrituras no son transaccionales: ante
fallo parcial se informa qué cambios anteriores pudieron aplicarse.

La primera prueba física encontró un problema al restaurar tipos `NamedInt` y
el orden modo/SmartShift. Ambos se corrigieron; el segundo roundtrip confirmó
restauración completa. Conservar esa regresión, no repetir el fallo histórico.

### 4.4 Fiabilidad, perfiles y rendimiento — completados

- Persistencia en `$XDG_CONFIG_HOME/linux-scrollmaster/profiles.json`, o
  `~/.config/linux-scrollmaster/profiles.json`, schema 1–10: botones nativos (2), modificadores (3), edición (4), gestos (5), reglas por aplicación (6), multimedia (7), unidades de distancia (8), atajos personalizados (9) y acciones de escritorio (10), sin inventar asignaciones en perfiles antiguos.
- Validación del documento, identidades, nombres, valores y referencias activas.
  Un archivo corrupto no se sobrescribe silenciosamente.
- Guardado temporal 0600, flush/fsync, reemplazo atómico y fsync del directorio.
  No se escriben perfiles con lecturas incompletas de los seis ajustes.
- Antes de editar manualmente o restaurar se desarma la reaplicación en disco.
  Si falla ese guardado no se escribe hardware; si falla hardware parcialmente,
  no se rearma un perfil antiguo que deshaga la intención del usuario.
- Restaurar y habilitar reaplicación son acciones separadas. La casilla puede
  armar un perfil guardado aunque el mouse esté apagado.
- `snapshot` compara el perfil activo contra lecturas reales y aplica diferencias.
  Devuelve una lista de claves escritas; solo una lista no vacía justifica el
  mensaje de recuperación con escrituras confirmadas.
- El worker reutiliza conexión y descubrimiento; invalida/reabre tras pérdida de
  conexión o error. Se restaura el hook de persistencia de Solaar al cerrar.
- El host serializa operaciones y sondea cada 5 s tras completar la anterior.
  Funciona minimizado mientras el proceso permanezca abierto, sin daemon externo.
- Timeout de respuesta 45 s; respuesta máxima 2 MiB y solicitud máxima 4096 bytes.
  Ante timeout/protocolo inválido se mata y recoge el hijo; se reinicia al consultar.
- Revisiones monotónicas ordenan eventos y respuestas. Se conserva identidad
  durante desconexión, se invalidan controles ante error y se recuperan al leer.
- Se corrigieron mensajes tardíos: un éxito IPC antiguo no anuncia restauración
  tras una desconexión posterior ni sustituye una confirmación más reciente.
- DPI numéricos con Aplicar/Enter, validación y slider conservado. El usuario
  confirmó que la experiencia mejoró tras denunciar latencia excesiva inicial.

Mediciones históricas de solo lectura: enumeración repetida 2,409 s; primera
lectura residente 2,648 s; siguientes lecturas completas 0,128 y 0,142 s.
No presentarlas como latencia medida de escritura o garantía de rendimiento.

### 4.5 GUI y permisos

Se creó una identidad propia, ilustración SVG del mouse, zonas seleccionables,
paneles contextuales, temas claro/oscuro, batería, estado y mensajes de error.
Se conserva el diseño actual. Los controles no implementados se identifican como
pendientes, no producen simulaciones de éxito.

La GUI y el puente corren como usuario normal. Se reutiliza acceso hidraw de las
reglas de Solaar ya instaladas. No se usa evdev/uinput. Alt/Ctrl usan el teclado virtual autorizado de Wayland.
Un flock por usuario coordina procesos propios; se mantiene entre solicitudes mientras hay modificadores. Se detectan procesos Solaar/logid.
Esto **no es exclusividad de kernel** frente a cualquier otro controlador.

Se resolvió el inicio WebKitGTK Error 71 con un ajuste exclusivo del proceso:
`WEBKIT_DISABLE_DMABUF_RENDERER=1` en Linux/Wayland con módulo NVIDIA y sin override
explícito. Está antes de crear GTK/threads. No se editó Hyprland ni se forzó X11.
Conservar el carácter acotado de esta solución y su referencia en compatibilidad.

## 5. Mapa de código y contratos

| Ruta / símbolo | Responsabilidad y punto de entrada |
|---|---|
| `src/App.vue` | Pantalla, controles, perfiles, temas y validación de DPI exactos |
| `src/style.css` | Identidad visual y distribución actual |
| `src/device.ts:useDevice` | invoke Tauri, listener `device-snapshot`, refresh de foco y limpieza |
| `src/deviceState.ts:createDeviceState` | Estado probado, selección, revisiones, errores, operaciones |
| `src-tauri/src/main.rs:hardware_request` | Mutex del backend, arranque/reinicio y revisiones; polling nativo |
| `src-tauri/src/bridge.rs:Worker` | Python aislado, JSON-lines, límites, timeout y kill/wait al salir |
| `bridge/service.py:execute` | Frontera con flock y conversión de errores a respuestas JSON |
| `bridge/service.py:handle` | Operaciones, persistencia y reconciliación de perfiles |
| `bridge/model.py:validate_document` | Modelo persistido 1–7 independiente del hardware |
| `bridge/solaar_adapter.py:SolaarBackend` | Enumeración, identidad, capacidades, lectura y `apply` |
| `bridge/solaar_adapter.py:ResidentAdapter` | Reutilización, eventos del mismo descriptor e invalidación |
| `bridge/modifiers.py` | Estado de Alt/Ctrl, IPC del guardián y cliente de biblioteca nativa |
| `native/keyboard.c` | Teclado virtual Wayland propio, combinaciones completas y cleanup de teclas |
| `bridge/actions.py` | Catálogo cerrado de acciones de edición, compartido por validación y salida |
| `src-tauri/tauri.conf.json` | Recursos explícitos del puente, ventana y CSP |
| `packaging/arch/` | PKGBUILD y entrada desktop aún sin instalación validada |

El comando Tauri recibe `{op, device?, changes?, name?}`. Operaciones permitidas:
`snapshot`, `set`, `remap`, `save`, `restore`, `auto`. No permite shell ni rutas arbitrarias.
Responde `{ok, data?, error?, revision}`; las revisiones las añade el host Rust.
`device` es Unit ID o serial estable, no slot del receptor ni nombre visual.
El worker usa `/usr/bin/python -I bridge/service.py --serve`.

Snapshot contiene dispositivos, perfiles, mapa `active`, warnings, diagnóstico
y reconciliaciones. Un navegador con `pnpm dev` no accede a hidraw: debe mostrar
el mensaje que pide abrir la app nativa, sin fixtures de dispositivos.

## 6. Evidencia física y preferencias del usuario

Registro detallado y horarios: `docs/reliability-milestone.md` y `docs/testing.md`.
No ejecutar otra prueba destructiva o repetir preguntas ya contestadas para
reconstruir esos resultados.

- Mouse probado: MX Master 3S B034; Bolt C548; Unit ID `<UNIT_ID>`; HID++ 4.5.
- Usuario aplicó 1850 DPI por GUI; lectura independiente confirmó el valor.
- Guardó **«Mi perfil»**, cambió a 1800 y restauró; volvió a 1850 con confirmación.
- Apagó el mouse dejando Bolt conectado; se verificó ausencia real del dispositivo.
- Armó el perfil offline con teclado; al encender, sin Restaurar/Actualizar, volvió
  el perfil y apareció la confirmación de escrituras efectivas.
- Después de reiniciar el equipo, abrir Linux ScrollMaster recuperó 1850 frente a lectura previa
  de 1000 y corrigió inversión lateral conforme al perfil persistido.
- SmartShift 11, alta resolución y giro libre se contrastaron con lecturas reales.
  El usuario verificó físicamente giro libre.
- Las inversiones se probaron y devolvieron por el usuario; se contrastó el estado
  final. No se capturaron independientemente todos los valores intermedios desde GUI;
  el roundtrip del backend sí cubrió escrituras y restauración de ambos ajustes.
- Reanudación con Linux ScrollMaster abierta: usuario confirmó funcionamiento y seis valores
  se leyeron correctamente después. No se afirma que hiciera falta reescribirlos.

### Último estado confirmado, no lectura en vivo

| Ajuste | Preferencia final |
|---|---|
| Perfil | «Mi perfil», reaplicación habilitada |
| DPI | 1850 |
| SmartShift | 10 |
| Modo | Dentado / 2 |
| Inversión vertical | false |
| Alta resolución | **true**, guardada deliberadamente por el usuario |
| Inversión lateral | true |

La última habilitación se hizo con el servicio; la casilla GUI ya había sido
probada offline. No volver a 1800 DPI o alta resolución false: eran valores
históricos anteriores a la elección del usuario. Comprobar estado actual antes
de una futura prueba, porque puede haberlo cambiado desde esta sesión.

## 7. Pruebas, ejecución y comprobaciones disponibles

Desde la raíz del proyecto:

```sh
pnpm test
pnpm build
cargo check --offline --manifest-path src-tauri/Cargo.toml
cargo test --offline --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
bash -n packaging/arch/PKGBUILD
pnpm tauri build --debug --no-bundle
git diff --check
```

Todos pasaron en el estado previo a este documento. Los cambios más recientes
de código añadieron dos regresiones Vue, que fallaron antes del arreglo y pasaron
después. Este plan no implica una nueva sesión de verificación de hardware.

| Grupo | Archivo | Cobertura actual |
|---|---|---|
| Python: 170 casos | `tests/test_*.py` | Contrato/versiones, recursos, perfiles, acciones, gestos, escritorio, validación, reconciliación y recuperación; incluye C real con transporte simulado |
| Estado Vue: 17 casos | `tests/device-state.test.ts` | Identidad estable, revisiones, desconexión, errores y recuperación, mensajes tardíos |
| Distancia y atajos: 9 casos | `tests/gesture-distance.test.ts`, `tests/shortcuts.test.ts` | Conversión explícita, límites, formato y ausencia de mutaciones |
| Componentes Vue: 8 casos | `tests/shortcut-components.test.mjs` | Borradores, Aplicar, sondeos, lectura confirmada, hardware/IPC no disponible, carga sin ejecución, intención de autostart y error |
| Estado de preferencias: 7 casos | `tests/desktop-preferences.test.ts` | Lectura sin activación, aislamiento de bandeja, concurrencia, errores y readback |
| Rust: 23 casos | tests de `src-tauri/src/` | Worker, timeout/drop, archivos de perfiles, instancia, bandeja y autostart con compatibilidad de nombre |
| Hardware optativo | `tests/hardware_roundtrip.py` | Seis cambios reales y restauración en finally; fuera de la suite regular |
| Observación independiente | `tests/hardware_observe.py` | Lecturas reales sin snapshot reconciliador; `--profiles-only` para archivo propio |

Las pruebas regulares usan dobles y procesos de prueba; no requieren mouse, root,
red ni permisos hidraw. Los casos C requieren compilador, pkg-config y cabeceras
Wayland/xkbcommon, ya necesarios para construir Linux ScrollMaster. Los componentes usan el
renderer Vue en memoria. No hay suite WebDriver de clics: la aceptación GUI fue
manual con usuario y contraste independiente del dispositivo.

`git diff --check` no revisa contenido sin seguimiento. Si aparecen archivos nuevos,
revisarlos también expresamente; no anunciar una revisión total
del repositorio únicamente por ese comando.

Desarrollo nativo: `pnpm tauri dev`. Binario con frontend integrado:
`./src-tauri/target/debug/linux-scrollmaster`, tras el build mostrado. No abrir duplicados.
Si faltan dependencias, leer requisitos del README; no instalarlas por iniciativa
propia con privilegios. El archivo lock de pnpm debe conservarse.

### Protocolo para futuras pruebas físicas

1. Explicar qué se cambiará y solicitar la interacción física necesaria al usuario.
2. Comprobar ausencia de Solaar/logid; coordinar Linux ScrollMaster si se usa un script externo.
3. Tomar baseline real de todos los ajustes y del perfil automático para no
   confundir reconciliación con una escritura manual.
4. Usar el observador de solo lectura para evidencia independiente. **snapshot
   puede escribir** si hay perfil activo, no es un observador neutral.
5. Restaurar temporalidades en finally y leer de nuevo. Conservar cualquier nueva
   preferencia que el usuario decida guardar expresamente.
6. Marcar por separado resultados físicos, automatizados con dobles y pendientes.

## 8. Implementación pendiente por hitos

Las casillas sin marcar están pendientes aunque exista una idea o feature
anunciada por el hardware. No marcarlas completas por haber dibujado la UI.
`[-]` marca lo que queda fuera de alcance por falta de hardware para validarlo
(solo hay un MX Master 3S con su receptor Bolt); no cuenta como pendiente.

### Hito N — independencia de Solaar, prioridad de desarrollo

Objetivo obligatorio, previo a ampliar el catálogo de funciones o declarar una
distribución independiente. El backend híbrido se conserva como transición hasta
verificar su reemplazo. Plan detallado: [backend nativo](docs/native-backend.md).

- [x] N1: inventario del contrato y garantías; codec y transporte hidraw/HID++ Rust,
  respuestas correlacionadas, único propietario y 22 pruebas sin Solaar/hardware.
  Biblioteca aislada; [evidencia, procedencia y límites](docs/native-transport.md).
- [x] N2: descubrimiento, identidad, capacidades y lecturas reales por Bolt;
  diagnóstico estrictamente de solo lectura, sin restauración automática.
  Aceptado con Solaar cerrado: tabla de emparejamiento, slot 2, identidad BLE y
  seis valores contrastados en dos diagnósticos; perfiles intactos. 29 pruebas.
  [Detalle, correcciones y límites](docs/native-diagnostic.md).
- [x] N3: seis ajustes y remapeo nativo, validación de capacidades, confirmación
  sin caché y pruebas físicas acotadas con restauración de valores originales.
  Seis setters y remapeo secundario implementados; 27 pruebas N3. DPI
  1800→1850→1800 y restauración completa verificados con autorización del usuario.
  Atrás→Adelante→Atrás restaurado y verificado; aceptación ampliada el 22 de octubre.
  [Ajustes](docs/native-settings.md) y [botones](docs/native-button-remapping.md).
- [x] N4: notificaciones, diversion, guardián, acciones y gestos con cancelación,
  liberación y restauración ante cierre, desconexión y fallo. Recepción pasiva y
  decoder implementados; FIFO conservada durante consultas/confirmaciones, con
  fallo cerrado al saturarse. Consumidor síncrono disponible durante consultas;
  propietario pasivo reúne refresh y cierre, aceptados en reposo. Motor de
  [restauración de reporting](docs/native-recovery.md) probado sintéticamente;
  [guardián pasivo independiente](docs/native-guardian.md) con lease continuo y
  recuperación probado con procesos/mouse sintético. [Motor de acciones/gestos](docs/native-actions.md)
  con [entrega directa interna](docs/native-action-delivery.md) probada en sesión y
  procesos sintéticos; [captura optativa confirmada](docs/native-capture.md) implementada.
  [Salida nativa de escritorio](docs/native-desktop.md) implementada con pruebas
  sintéticas. Aceptado físicamente el 22 de octubre (captura, gestos, SIGKILL).
  [Propietario](docs/native-owner.md).
  [Alcance y límites](docs/native-events.md).
- [x] N5: servicio nativo de perfiles/reconciliación e integración Tauri; preservar
  schemas 1–10, datos, contratos y acciones de escritorio existentes.
  [Núcleo de perfiles y persistencia](docs/native-profiles.md) implementado:
  validación/gestión local, revisión y guardado confirmado, con pruebas en temporales.
  [Servicio y reconciliación síncronos](docs/native-service.md) implementados para
  ajustes/remapeos nativos. [Dueño con reconexión](docs/native-profile-owner.md),
  arranque offline y lease implementados. [Worker JSON-lines](docs/native-worker.md)
  separado y supervisor compartido implementados. [Foco/catálogo](docs/native-focus.md)
  ya conectan reglas con consultas acotadas. [Captura residente](docs/native-profile-capture.md)
  con transiciones de plan bajo el mismo dueño y reconexión tras limpieza confirmada.
  **Implementación completa:** [GUI con backend nativo](docs/native-gui.md) optativo
  (`SCROLLMASTER_BACKEND=native`), localización, presentación y gestión offline.
  [Aceptación física](docs/native-acceptance.md) de bloques 1–5 el 22 de octubre.
- [ ] N6: retirar Solaar/Python del runtime y paquete; reglas hidraw propias
  acotadas y aceptación desde instalación limpia sin Solaar. **Implementado:**
  paquete `arch-native` sin Python/Solaar y regla udev limitada a la interfaz
  HID++ (02). `makepkg` verificado sin instalar (22 de octubre): paquete sin
  Python/Solaar, compilado con `native-default`. Pendiente: instalación limpia,
  aceptación y cambio de default.

Cierre: funciones migradas y regresiones comprobadas sin Solaar instalado ni
invocado, sin Python en runtime, sin permisos globales y sin pérdida de perfiles.
Bluetooth y escenarios no probados se declaran pendientes, no se hereda la
evidencia del adaptador anterior como prueba del transporte nuevo.

Siguiente: ejecutar el [protocolo de aceptación física](docs/native-acceptance.md)
(N3 ampliado, N4 captura/acciones/recuperación, N5 transiciones/foco, N6 instalación
limpia) y, tras aceptarlo, cambiar el backend por defecto y retirar `bridge/`.
Asignaciones y activaciones
de diversion/raw XY ya tienen confirmación y pruebas sintéticas, con consumidor y
recuperación preparados antes de escribir; no cuentan como aceptación física.
El guardián pasivo requiere aceptación física; la aceptación ampliada N3 sigue pendiente.
No introducir un segundo controlador ni activar un backend incompleto por defecto.

### Hito A — ampliar cobertura del MVP existente

No bloquea la aceptación Bolt ya conseguida. Puede avanzar en paralelo temporal
al siguiente desarrollo cuando el usuario disponga del transporte o entorno.

- [ ] Probar Bluetooth real: detección, lectura, seis escrituras, perfiles y
  reconexión. Verificar identidad frente al mismo mouse por Bolt; no asumirla.
- [ ] Probar extracción/reinserción del receptor, no solo interruptor del mouse.
- [-] Probar varios receptores/dispositivos y selección explícita; revisar que
  nunca se aplique un perfil a otra identidad. **Fuera de alcance:** solo hay un
  MX Master 3S y un receptor Bolt disponibles (decisión del 22 de octubre de 2026).
  El código ya rechaza más de un receptor y valida la identidad (pruebas sintéticas).
- [ ] Probar desconexión durante una escritura y recuperación posterior sin
  confirmaciones engañosas ni bloqueo permanente del worker.
- [ ] Ampliar pruebas de permisos en un entorno preparado sin quitar ACL del
  entorno de trabajo. Fallos reales de disco solo en fixtures/entorno desechable.
- [ ] Revisar accesibilidad por teclado/lector, foco, etiquetas, temas y escalas.

Cierre: registrar versiones, transporte, casos y evidencia concreta. Los casos
sin hardware permanecen pendientes sin mostrar una compatibilidad ficticia.

### Hito B — botones y gestos reales, implementado con aceptación parcial por función

Primer incremento implementado: asignación nativa del firmware para Atrás, Adelante
y botón de gestos, catálogo real, confirmación sin caché y perfiles schema 2/3.
Alt/Ctrl mantenidos en los laterales ya tienen consumidor HID++, teclado virtual
Wayland y guardián. Usuario confirmó ambos modificadores y su liberación; cierre
y SIGKILL restauraron reporting real. Las cuatro direcciones y la mejora de precisión
tienen aceptación física; resta ampliar cobertura de perfiles y desconexiones.
Detalles: `docs/native-buttons.md` (primera fase) y `docs/held-modifiers.md` (vigente).

- [x] Lectura real del catálogo y de asignaciones nativas sin diversion.
- [x] Operación remap, controles GUI y perfil 2 con compatibilidad para perfil 1;
  probados automáticamente, pendientes de aceptación física de escritura.
- [x] Completar aceptación física de asignación, restauración y perfil con botones
  (backend nativo, 22 de octubre: remapeos, perfiles con botones y reaplicación).
- [x] Alt/Ctrl seleccionables y funcionales en ambos laterales, sin asignación automática.
- [x] Consumidor único HID++, modificadores mantenidos y liberación real verificados
  con el usuario; guardián restauró botones al cerrar y tras SIGKILL del worker.
- [x] Schema 3 para perfiles con modificadores, compatible con 1/2 y probado con dobles.
- [ ] Aceptación física de perfiles schema 3/4 al guardar/restaurar/reconectar.
- [x] Acciones de edición cerradas en laterales y schema 4; Copiar/Pegar ya
  verificados físicamente, junto con Cortar/Deshacer, en el campo GUI nativo.
- [x] Errores idle del motor visibles tras recuperar el backend; se elimina el
  mensaje antiguo de éxito y se permite reintentar la asignación.
- [ ] Prueba física de desconexión mientras se mantiene un modificador.

Dependencias: preservar fiabilidad actual, diseño de eventos y recuperación
antes de activar diversion.

- [x] Investigar selectivamente controles HID++ 1B04 del dispositivo real, eventos
  y raw XY mediante código local de Solaar/documentación primaria actual.
  Documentar IDs y capacidades leídos; nunca pedir al usuario escribirlos.
- [x] Definir contrato de eventos distinto de las respuestas de configuración:
  propietario del transporte, orden, presión de cola, desconexión y cancelación.
  Motor interno separado del JSON externo; sin stream genérico hacia GUI.
  No introducir lectores concurrentes de hidraw que se roben respuestas.
- [x] Consumir notificaciones de pulsar/soltar en el worker y durante consultas HID++.
  El JSON-lines externo sigue siendo solicitud/respuesta: el motor de modificadores
  consume dentro del servicio; aún no existe stream genérico de eventos hacia la GUI.
- [x] Registrar estado previo y diseñar activación/desactivación segura de diversion.
  Cubrir cierre normal, error, desconexión y límites de recuperación tras crash.
- [x] Implementar motor de acciones independiente de Vue y Solaar: pulsar/soltar,
  cancelación y estado consistente; preservar botones principales utilizables.
- [x] Implementar gestos direccionales con botón mantenido, umbral, dirección,
  ausencia de gesto y cancelación. Definir unidades y límites explícitos.
- [x] Conectar botones laterales y botón de gestos a acciones reales desde GUI.
  Ampliar perfiles con migración versionada sin perder los seis ajustes actuales.
- [ ] Añadir pruebas unitarias del motor, secuencias de eventos, pérdida de conexión,
  recuperación de diversion y aceptación física con el usuario.

Cierre: una asignación configurada desde GUI produce la acción real en Wayland;
soltar/cancelar no deja botones ni teclas retenidos; cierre devuelve control del
mouse. La capacidad anunciada del dispositivo no basta como verificación.

### Hito C — acciones Wayland e integración Hyprland

Parte de este hito será dependencia de la salida de acciones del hito B. Implementar
primero una acción mínima verificable; después ampliar el catálogo.

- [x] Evaluar mecanismo de salida para atajos: IPC del compositor para acciones
  específicas y portales/libei/uinput para teclas genéricas según viabilidad.
  Implementado teclado virtual Wayland; sin helper privilegiado. Otros mecanismos pendientes.
- [ ] Si hacen falta permisos, diseñar el mecanismo mínimo y presentar el cambio
  concreto al usuario antes de instalarlo. GUI nunca root; no acceso global al teclado.
- [x] Adaptador Hyprland para foco y cuatro acciones de espacios/ventanas. Socket
  nuevo y firma del entorno resuelta en cada conexión; versión y funciones Lua
  comprobadas antes de diversion y al ejecutar. No edita configuración personal.
  Aceptación física pendiente; firma antigua tras reiniciar compositor exige reabrir.
- [x] Catálogo gráfico multimedia para gestos: seis acciones implementadas.
- [x] Editor de atajos propios en laterales y gestos, sin comandos ni códigos HID
  visibles. Catálogo validado, guardián, schema 9 y componentes probados; aceptación
  física pendiente. Ver `docs/custom-shortcuts.md`.
- [x] Espacio anterior/siguiente y alternar pantalla completa/maximizado por IPC
  Hyprland, con guardián y schema 10. Ver `docs/desktop-actions.md`.
- [ ] Otras acciones del sistema y adaptadores para otros compositores.
- [ ] Probar liberación de todas las teclas/botones sintetizados ante cancelación,
  fallo del backend y desconexión del mouse.
- [ ] Mostrar límites por compositor y distinguir rutas Wayland/X11 verificadas.

Cierre: atajos y acciones elegidas funcionan en Hyprland real con permisos mínimos
y cancelación fiable. No usar soluciones exclusivas X11 como soporte Wayland.

### Hito D — perfiles por aplicación y gestión completa

- [x] Modelo versionado de reglas por aplicación con migración de schema 1.
- [x] Identificar aplicación activa mediante adaptador del escritorio; resolver
  ausencia de foco, ventanas desconocidas y cambios rápidos de aplicación.
- [x] Definir precedencia entre perfil global, por aplicación y edición manual.
  Evitar que reglas de foco reviertan inmediatamente una intención manual.
- [x] GUI para crear/seleccionar reglas sin introducir class/app_id manualmente
  cuando pueda elegirse la aplicación desde una lista comprensible.
- [x] Importación/exportación, renombrado y eliminación implementados con pruebas
  de persistencia y archivos. Preview, duplicados sin sobrescribir, referencias
  activas consistentes y gestión offline. Falta aceptación gráfica de diálogos.
- [ ] Pruebas de migración, reglas, cambio de foco y aceptación real.

Cierre: cambio de aplicación produce la configuración/acción esperada y el retorno
al perfil global es predecible; ningún error destruye los perfiles existentes.

### Hito E — ciclo de vida de escritorio

- [ ] Diseñar servicio de sesión para operar sin ventana abierta, si se mantiene
  esa decisión de producto. Actualmente cerrar Linux ScrollMaster detiene la reconciliación.
- [x] Definir una sola instancia controladora y un IPC local con límites explícitos.
  Una futura GUI y servicio no deben abrir dos controladores HID++ simultáneos.
- [ ] Evaluar eventos udev/hotplug frente al sondeo y fallback de reconexión.
- [x] Inicio automático XDG voluntario desde GUI, con archivo propio canónico y
  lectura confirmada. Conserva entradas ajenas/editadas, no cambia bandeja/perfiles
  y no se habilitó en esta sesión. Aceptación real al iniciar sesión pendiente;
  Hyprland por sí solo no garantiza procesar XDG. Ver `docs/autostart.md`.
- [x] Bandeja opcional, cierre versus minimizar explicado, recuperación de errores.
  Implementación y pruebas automáticas listas; aceptación gráfica pendiente.
- [ ] Pruebas de arranque/cierre, reinicio del worker, sesiones y recursos liberados.

Cierre: comportamiento de ventana/servicio y autostart es explícito, reversible
y probado; no prometerlo mientras solo exista polling dentro de Tauri.

### Hito F — empaquetado y distribución

- [ ] Revisar y versionar únicamente fuentes libres; generar tarball limpio.
  No empaquetar el workspace completo ni usar `git archive HEAD` antes del primer commit.
- [ ] Sustituir checksum `SKIP` de desarrollo, fijar fuente/versiones y revisar
  licencias transitivas; el paquete final debe cumplir N6 sin Solaar/Python.
- [ ] Probar build release y rutas de recursos fuera del workspace. Durante la
  transición, verificar `/usr/lib/linux-scrollmaster/bridge/`; tras N6, retirar
  recursos Python y verificar las rutas del backend nativo.
- [ ] Construir en chroot limpio; probar instalación, entrada desktop, icono,
  lectura/escritura y desinstalación conservando perfiles del usuario.
- [ ] Documentar instalación y actualización Arch sin instrucciones privilegiadas
  innecesarias ni reglas 0666 para entrada.
- [ ] Evaluar otros formatos con backend nativo, acceso hidraw y reglas del host.
  AppImage/Flatpak no son actualmente distribuciones autosuficientes verificadas.

Cierre: paquete instalable reproducible, sin binarios propietarios, probado fuera
del checkout y con rutas/dependencias verificadas. No confundir PKGBUILD con release.

### Hito G — funciones futuras, fuera del MVP actual

- [ ] Macros/secuencias con tiempos, cancelación y liberación de estado.
- [-] Otros modelos mediante capacidades, sin eliminar filtros a ciegas. **Fuera de
  alcance:** sin otros modelos disponibles para validar (22 de octubre de 2026).
- [ ] Alternativas locales a Smart Actions con reglas comprensibles y verificables.
- [ ] Estudiar Flow: descubrimiento de equipos, autenticación, red, transferencia
  y portapapeles consentidos. No confundir Easy-Switch con Flow ni prometer
  interoperabilidad con el protocolo propietario.

## 9. Riesgos técnicos y límites conocidos

- API interna Solaar: dependencia transitoria de la versión probada, pendiente
  de retirar en Hito N. El backend nativo todavía no está implementado.
- Ausencia de memoria permanente garantizada: firmware puede resetear ajustes;
  reaplicación necesita Linux ScrollMaster abierta y el perfil habilitado.
- Reconexión: detección inicial/reapertura puede tardar segundos; sondeo cada 5 s
  más duración de consultas. Una consulta ya iniciada puede retrasar recuperación.
- Concurrencia: flock coopera entre procesos propios; comprobación de nombres de
  procesos no excluye controladores desconocidos o que arranquen después.
- Lotes de escritura: no hay rollback transaccional garantizado de hardware.
- Hay consumo de eventos, Alt/Ctrl y cuatro acciones de edición en Wayland; hay
  gestos direccionales con izquierda/derecha verificados y multimedia con aceptación
  parcial. Los atajos personalizados están implementados, pendientes de aceptación
  física; usan keymap US y dependen de la aplicación/compositor. Sin helper privilegiado.
  Solo un mouse con modificadores a la vez.
- Acciones de espacios/ventanas limitadas a Hyprland 0.55/0.56 Lua. Capacidades
  reales comprobadas; aceptación GUI/mouse pendiente. Un timeout no se reintenta,
  porque la acción podría haberse ejecutado. No hay dispatcher/Lua arbitrario.
- Inicio automático depende de un escritorio que procese XDG; no se instala un
  servicio para suplirlo. Una ruta movida requiere actualización explícita. Las
  pruebas no sustituyen el próximo inicio de sesión real.
- No hay certificación de compatibilidad Bluetooth/X11/otros escritorios por el
  mero hecho de que el diseño tenga rutas para ellos.

## 10. Cómo cerrar un incremento y mantener este plan

Para cada entrega, añadir qué código cambió, por qué, pruebas ejecutadas y alcance
real de la evidencia. Distinguir siempre:

1. Implementado y probado automáticamente.
2. Implementado y verificado con hardware/usuario.
3. Implementado pero pendiente de prueba física.
4. Diseñado o pendiente, sin implementación.

Actualizar tabla de estado, casillas del hito afectado y documentación específica.
Si cambia el número de pruebas, conservar coherencia entre este archivo,
`docs/testing.md` y `docs/reliability-milestone.md`. Registrar decisiones nuevas
en arquitectura; no duplicar logs extensos ni pseudocódigo en el plan.

### Documentos de referencia

- `README.md`: instalación, ejecución, uso habitual y límites visibles.
- `AGENTS.md`: instrucciones obligatorias de trabajo.
- `docs/architecture.md`: capas, contratos y decisiones vigentes.
- `docs/compatibility.md`: matriz de plataformas, transportes y subsistemas.
- `docs/reliability-milestone.md`: aceptación del ciclo real y preferencias finales.
- `docs/testing.md`: pruebas iniciales, pruebas actuales y cobertura pendiente.
- `docs/native-buttons.md`: primera fase de botones nativos y versión Solaar.
- `docs/held-modifiers.md`: Alt/Ctrl funcionales, guardián y evidencia física.
- `docs/button-shortcuts.md`: acciones de edición, schema 4 y evidencia vigente.
- `docs/custom-shortcuts.md`: editor de combinaciones, schema 9 y pruebas con transporte simulado.
- `docs/desktop-actions.md`: cuatro acciones Hyprland, capacidades, plazos y schema 10.
- `docs/native-desktop.md`: salida Rust de esas cuatro acciones, preflight, límites,
  cierre y pruebas sintéticas del motor/guardián; aceptación física pendiente.
- `docs/native-profiles.md`: schemas 1–10 y gestión/persistencia Rust, revisión,
  conflictos y límites del núcleo de perfiles; integración Tauri pendiente.
- `docs/native-service.md`: guardado, desarmado y reconciliación por petición con
  backend Rust nativo; alcance del guardián/worker pendiente y evidencia sintética.
- `docs/autostart.md`: inicio XDG voluntario, entrada propia, límites y pruebas temporales.
- `docs/roadmap.md`: resumen de producto; este plan desarrolla la ejecución.
- `packaging/arch/README.md`: preparación de distribución, todavía no publicada.
- `docs/suspend-incident.md`: archivo histórico; no genera pendientes del proyecto.

### Último incremento: modificadores mantenidos

Alt/Ctrl fueron primero placeholders deshabilitados; el usuario pidió corregirlo.
Ya están implementados y verificados físicamente en ambos laterales. No repetir
la implementación de ese motor como si estuviera pendiente. La prueba temporal
terminó con funciones nativas restauradas y la app reabierta; perfiles intactos.
Lectura final 02:17:22 UTC: 1850 DPI, SmartShift 10, modo 2, alta resolución false;
«Mi perfil» conserva alta resolución true y está desarmado. No imponer estados
históricos al usuario. Evidencia y límites: `docs/held-modifiers.md`.

### Último incremento: acciones de edición

Copiar, Pegar, Cortar y Deshacer se ofrecen como acciones reales en los laterales,
con guardián, flancos de pulsación y combinaciones completas. El usuario confirmó
las cuatro acciones; al terminar devolvió ambos botones a sus funciones originales.
La lectura independiente 03:23:15 UTC confirmó ese retorno y el perfil intacto. Suite de 99 pruebas y
compatibilidad con perfiles anteriores. No confundir el catálogo cerrado con un
editor de atajos arbitrarios ni con gestos direccionales implementados.
Ver `docs/button-shortcuts.md`. No guardar ni armar perfiles sin indicación del usuario.

### Gestor de perfiles completo

Importar/exportar, renombrar y eliminar ya tienen backend, UI y pruebas. Los
diálogos GTK no bloquean el worker HID++. No reimplementar estas operaciones;
completar su aceptación gráfica cuando se pruebe el siguiente ejecutable.
Ver `docs/profile-management.md`. Límite 100 perfiles/128 KiB e IPC 256 KiB.

### Gestos direccionales

Motor puro, editor visual y transporte raw XY con recuperación integrados. Schema 5
preserva gestos en perfiles e importaciones. Aceptación física pendiente: no asumirla
por las pruebas de paquetes sintéticos. Ver `docs/directional-gestures.md`.

### Perfiles por aplicación en Hyprland

Reglas schema 6, adaptador IPC de solo lectura y selector de aplicaciones abiertas
implementados. Foco y catálogo contrastados con Hyprland real (cinco aplicaciones).
13 pruebas nuevas; aceptación GUI con cambios del mouse pendiente. Precedencia y
límites en `docs/application-profiles.md`. No se crearon reglas del usuario.

Captura raw XY y restitución tras cierre/SIGKILL verificadas en MX Master 3S real
con `hardware_modifiers_roundtrip.py --gestures`. Perfiles intactos. Movimiento
y ejecución de gestos por el usuario desde GUI siguen pendientes.

### Ciclo de escritorio

Instancia única y bandeja opcional implementadas; seis pruebas Rust nuevas. La
bandeja está desactivada por defecto. El segundo inicio recupera la ventana.
No se habilitó autostart. Ver `docs/desktop-lifecycle.md`; aceptación GUI pendiente.

Aceptación de gestos: el usuario confirmó Copiar/Pegar mediante izquierda/derecha
desde la GUI. Captura, movimiento y salida reales verificados; pendientes otros
sentidos y controles multimedia solicitados después de esa aceptación.

### Gestos multimedia

Seis controles en el selector, catálogo compartido con backend, salida sin Ctrl y
schema 7. Cinco regresiones nuevas; aceptación física pendiente. Detalles en
`docs/media-gestures.md`. La app nueva está abierta para esa prueba.

### Precisión de gestos e investigación adicional solicitada

Usuario confirmó multimedia, pero reportó reconocimiento de dirección deficiente.

Solaar documenta primer raw XY anómalo en 1B04 >=5; el dispositivo real anuncia5.
Adaptador corregido y clasificador con dirección fijada/tolerancia 1,2; cancelación
requiere liberación física antes de rearmar. Cinco regresiones nuevas. Aceptación
física de la corrección pendiente; no afirmar equivalencia con Windows.

Build release offline completado. Kit Arch de bf47aa9 generado dos veces con
checksum idéntico y `makepkg --verifysource` aprobado: no instalado ni probado
en chroot. Sus 87 miembros no incluyen EXE/DLL/MSI/SO propietarios. Regenerar
desde un commit actual antes de distribuir; el kit anterior es evidencia histórica.

Aceptación de precisión: el usuario confirmó «ahora funciona mucho mejor» tras
el filtro inicial de 1B04 v5 y la estabilización de la dirección. La recuperación
real cierre/SIGKILL volvió a pasar sin cambiar perfiles. No prometer identidad
con Windows; normalización DPI sigue pendiente.

Una repetición final detectó un reinicio de instancia intermitente: corregido con
unlock explícito y reintento del lease durante cierre. Regresiones para descriptor
duplicado y propietario que sale durante activación; suite final 162 pruebas.

### Cierre de esta continuación

Commits por bloques: botones/acciones; gestor de perfiles; gestos; reglas de
aplicación; recuperación real; instancia/bandeja; multimedia;
precisión; bloqueo de instancia; compilación atómica y acceso a preferencias.
El código propietario nunca se ejecutó ni se añadió a Git.

Prioridades para retomar: completar aceptación GUI de bandeja, reglas y diálogos;
verificar perfiles schema 3–8 al reconectar y aceptar físicamente la distancia
estable ya implementada. El editor de atajos se añadió en el incremento siguiente;
después se añadieron cuatro acciones del escritorio e inicio automático XDG
voluntario. Quedan las aceptaciones pendientes. Bluetooth y chroot/instalación requieren
pruebas específicas. No declarar ninguno completado por compilar o usar dobles.


### Distancia estable por DPI — incremento actual

Implementados motor de distancia estimada, conversión explícita en GestureEditor,
helper puro `src/gestureDistance.ts` y schema 8. No migrar ni sobrescribir el perfil
schema 7 existente. La frontera Tauri y la identidad visual se conservan. Escala
actualizada con lecturas DPI reales; cambio durante pulsación cancela y exige
liberar; pérdida de DPI restituye botones y muestra error. No hay consultas HID
por paquete. Detalles y límites: `docs/gesture-distance.md`.

178 pruebas aprobadas y compilación debug integrada. Prueba real autorizada
`--physical-distance` aprobó DPI temporal, lectura, captura, cierre y crash del
worker, finally con DPI original confirmado y perfiles intactos. Linux ScrollMaster se abrió
con la versión nueva para comparar recorridos a 1850 y 3700 DPI. Aceptación física
y ciclo GUI guardar/restaurar/reconectar schema 8 pendientes; no confundir esta
prueba de recuperación con una calibración física del gesto.

### Atajos personalizados — incremento actual

Revisión del plan y código: gestos, gestión de perfiles, reglas y bandeja ya tenían
implementación. Se continuó el hito C con una combinación propia por lateral o
acción de gesto. Ctrl/Alt/Shift/Super, letras, dígitos, F1–F12 y navegación, con
catálogo anunciado por el adaptador, validación en servicio/motor/guardián/C y
pulsación/liberación completas. No se añaden macros, comandos ni captura global.

Se separaron selector compartido, editor de combinación y borrador de asignación;
los sondeos no aplican ni descartan la edición. Schema 9 conserva combinaciones
sin migrar perfiles anteriores. Se mantiene el teclado US propio; el resultado
depende de los atajos de la aplicación y de los reservados por el compositor.

199 pruebas aprobadas y compilación debug integrada. `pnpm test`, `pnpm build`,
`cargo check --offline`, `cargo test --offline` y revisiones de formato pasan.
No se abrió la GUI ni se modificaron hardware, perfiles reales o configuración
del escritorio. Aceptación física del editor y guardar/restaurar/reconectar schema
9 pendientes, junto con las aceptaciones previas de schema 8, reglas, diálogos y
bandeja. Detalles: `docs/custom-shortcuts.md`.

### Acciones de escritorio — incremento actual

Cuatro acciones para laterales y gestos: espacio anterior/siguiente y alternar
pantalla completa/maximizado. El catálogo se valida en servicio/motor/guardián;
el adaptador exige capacidades antes de diversion y el guardián ejecuta IPC sin
sintetizar teclas. No hay argumentos arbitrarios, shell ni edición del escritorio.

La sesión anuncia Hyprland 0.56.2. La consulta real de versión y la comprobación
fija sin efectos de funciones Lua aprobaron, sin cambiar espacios ni ventanas.
Soporte acotado a 0.55/0.56 Lua; otras versiones se deshabilitan explícitamente.
Plazo total de 1 s por intercambio, respuesta acotada y sin reintento de acciones.
Schema 10 conserva asignaciones sin migrar perfiles anteriores. La GUI explica
límites y deshabilita opciones sin soporte.

215 pruebas aprobadas: 170 Python, 17 de estado Vue, 5 de distancia, 4 de formato
de atajos, 6 de componentes y 13 Rust. Frontend, host y ejecutable debug integrado
compilados; formato y revisiones pasan. No se cambió el mouse ni perfiles reales.
Aceptación GUI/física de las cuatro acciones y perfiles schema 10 pendiente;
siguen pendientes schema 8/9, bandeja, reglas y diálogos. Después: inicio automático
voluntario, sin habilitarlo durante desarrollo. Ver `docs/desktop-actions.md`.

### Inicio automático XDG — incremento actual

Opción «Abrir Linux ScrollMaster al iniciar sesión» integrada en Preferencias, separada de
bandeja y perfiles. Rust deriva ruta y ejecutable, gestiona solo una entrada propia
canónica con publicación atómica y lectura posterior, y conserva archivos ajenos,
editados o enlaces. La GUI nunca pasa rutas ni comandos. Actualizar una ubicación
antigua también exige una acción explícita. El escritorio debe procesar XDG;
no se modifica Hyprland/Omarchy ni se instalan servicios.

233 pruebas aprobadas: 170 Python, 17 del estado Vue, 5 de distancia, 4 de formato
de atajos, 8 de componentes, 7 de estado de preferencias y 22 Rust. Los 18 casos
nuevos usan temporales/transportes de prueba; GLib/GIO valida archivos sin lanzarlos.
La ruta con `%` se rechazó tras detectar incompatibilidad del lector. La suite
Rust completa pasó fuera del sandbox para permitir sockets Unix temporales.

Frontend, host y ejecutable debug integrado compilados, formato/revisión aprobados.
No se habilitó autostart real ni se modificaron perfiles, bandeja, mouse o servicios.
Aceptación GUI y cierre/inicio de sesión pendientes, junto con las aceptaciones
anteriores. Prioridad siguiente: comprobar esos flujos con el usuario; después
accesibilidad y validación de distribución. Detalles en `docs/autostart.md`.

## N1: primer incremento del backend nativo (20 de octubre de 2026)

Revisados arquitectura, roadmap, contrato y prioridades. Implementada biblioteca
Rust experimental con codec HID++ corto/largo, correlación por los cuatro campos,
errores 0x8f/0xff y cola ordenada/acotada para informes intercalados. Transporte
Linux propio con descriptor no bloqueante y `poll`. Una sola petición en curso;
timeout, framing inválido, desconexión y fallo de escritura cierran la sesión sin
reenvío automático. El inventario preserva las obligaciones de perfiles, acciones,
guardado atómico, revisiones y recuperación para las siguientes etapas.

**256 pruebas aprobadas**, incluidas 45 Rust (23 existentes y 22 nuevas).
`pnpm test`, `pnpm build`, `cargo check/test --offline`, formato Rust y revisión
de diff aprobados. Los tests de E/S usan sockets Unix locales fuera del sandbox
porque este bloquea opciones de socket; no requieren root ni acceso al mouse.
Fixtures sintéticos con fuentes primarias y procedencia explícita. No se abrió
la GUI, no se accedió al hardware y no se cambiaron perfiles ni configuración.

N1 no está conectado al supervisor activo. No declara independencia del producto
ni latencia física del driver verificada. Siguiente: N2, validar interfaces Bolt,
identidad/features/lecturas y diagnóstico de solo lectura; no reutilizar `snapshot`
para ello, porque puede reaplicar perfiles. Detalles: [transporte nativo](docs/native-transport.md).

## N2: diagnóstico nativo de solo lectura (20 de octubre de 2026)

Implementados descubrimiento pasivo, parser acotado del descriptor HID, selección
explícita de interfaz/slot y verificación de metadata e ioctls antes de consultar.
Lease compatible con el servicio existente y comprobación de Solaar/logid/GUI y
worker Python. Catálogo privado de getters, identidad por Unit ID/modelo/nombre,
batería y seis lecturas con opciones DPI anunciadas. No hay setters, diversion,
perfiles, reconciliación, fallback a Solaar ni conexión a la GUI. El diagnóstico
optativo se ejecuta en un hijo supervisado con salida y tiempo acotados.

**280 pruebas aprobadas**, incluidas 69 Rust (24 nuevas de N2). `pnpm test`,
`pnpm build`, `cargo check --offline --all-targets`, `cargo test --offline`, formato
y diff aprobados. Tests con temporales y emulador sintético que inspecciona las
consultas para detectar setters; sin acceso al mouse en la suite regular.

`--list` identificó la interfaz Bolt `/dev/hidraw7` real y excluyó teclado/mouse
convencionales. El primer `--read` dentro del sandbox falló al abrir el bloqueo
de sesión por filesystem de solo lectura. El intento autorizado fuera detectó
Solaar y se detuvo antes de abrir hardware. No se lo cerró, no se mutó hardware y
no se tocaron perfiles ni configuración. La vista de procesos restringida no
acredita ausencia de controladores en el host.

En ese primer incremento N2 quedó sin marcar: faltaba aceptar ping/identidad/batería/capacidades/valores
y tiempos reales con Solaar/logid/GUI cerrados. Se solicitó al usuario cerrar
Solaar si desea realizar esa aceptación. N3 sigue pendiente y no se habilita por
haber pasado el emulador. Evidencia, uso, fuentes y límites del driver:
[diagnóstico nativo](docs/native-diagnostic.md).

## Aceptación real N2 y primeros setters N3 (20 de octubre de 2026)

El usuario cerró Solaar y encendió/movió el mouse. El guard de procesos pasó.
El slot 1 devolvió recurso 09; se implementó lectura fija del registro de
emparejamientos, que localizó B034 en el slot 2 (16 ms). Root usa ahora informes
cortos documentados. El mouse anunció modelo BLE, no eQuad: se corrigió esa
validación y el fixture de respuesta larga a consulta corta, con regresiones.

Dos diagnósticos nativos completos aprobaron en 1348/865 ms, Unit ID <UNIT_ID>,
protocolo 4.5, batería 25 %, DPI 1800, SmartShift 10, modo 2, inversión/resolución
vertical false e inversión horizontal true. Decodificación independiente de raw
y rango DPI coincide. Hash de perfiles antes/después de la repetición idéntico:
`2d83fccaec0f7632efec9f8e9a5920b0e33a1df72532926d21447fced47a93bd`.
N2 queda aceptado para estas lecturas Bolt, sin atribuir soporte Bluetooth ni
aceptación de escrituras. No imponer el baseline histórico de 1850 DPI.

Continuación N3: `native/settings.rs` valida identidad, estructura y capacidades
del lote completo; escribe solo diferencias y confirma con lectura completa sin
caché después de cada setter por el mismo lector. Orden modo/SmartShift,
preservación de bits y errores parciales explícitos sin reenvíos. Herramienta
separada `native_settings`, supervisada y voluntaria; no se conecta a la GUI,
no carga/desarma perfiles ni activa diversion. Remapeo queda pendiente.

**295 pruebas aprobadas**: 84 Rust (cinco nuevas de cierre N2 y diez de N3).
`pnpm test/build`, `cargo check --offline --all-targets`, suite Rust completa,
formato y diff aprobados; ambos ejemplos compilados. Las pruebas regulares usan
emulador/temporales y no acceden al mouse. El usuario autorizó la prueba física:
DPI 1800→1850→1800 confirmado, restauración en finally y seis valores iguales al
baseline. Lectura independiente posterior aprobada en 1253 ms, perfiles con el
mismo hash y sin journal pendiente. Otros cinco setters y remapeo aún pendientes
de aceptación nativa; no extender esta prueba a crash ni reconexión.
Detalles: [N2](docs/native-diagnostic.md) y [N3](docs/native-settings.md).

## Remapeo secundario nativo N3 (20 de octubre de 2026)

`native/buttons.rs` y `native_buttons` añaden catálogo 1B04 y remapeo de fuentes
83/86/195, destinos por grupos, bloqueo de cualquier reporting interceptado,
validación completa previa y confirmación sin caché. No activan diversion.
Diecisiete pruebas nuevas elevan la suite a **312 aprobadas**, incluidas 101 Rust;
frontend, build, check de todos los targets y suite Rust pasan.

El usuario autorizó la prueba Atrás→Adelante→Atrás. El setter y la restauración
pasaron; una lectura independiente confirmó las tres funciones originales y flags
cero, junto con los seis ajustes originales. El hash de perfiles no cambió y el
journal se eliminó al confirmar el baseline. No se verificaron pulsaciones en
aplicaciones. N3 sigue pendiente de otros setters y aceptación ampliada; N4 requiere
guardián antes de diversion. [Contrato, procedencia y límites](docs/native-button-remapping.md).

## Inicio N4: observación pasiva (20 de octubre de 2026)

`native/events.rs` y `native_events` añaden observación finita y decodificación
1B04 sin setters: slot/feature/software ID, botones con sincronización inicial,
flancos por conjuntos y raw XY con signo/contexto. Límites de 128 eventos y 4096
informes, cancelación y errores conservan evidencia parcial y limpian estado lógico.
`Session::receive` rechaza éxitos tardíos; timeout quieto sigue siendo normal.

**328 pruebas aprobadas**, incluidas 117 Rust: quince casos nuevos de eventos y
una regresión de lectura idle tardía. Build, check de todos los targets y suite
completa pasan. El sandbox bloqueó sockets Unix de tres pruebas del host; la
repetición con acceso a esos sockets pasó sin hardware en la suite regular.

Observación física de solo lectura: <UNIT_ID>/slot 2, 1000 ms, plazo completado,
cero informes y cero eventos; perfiles con el mismo hash. No acredita eventos
físicos ni recuperación. Los getters iniciales aún descartan notificaciones; no
usar este observador como servicio de diversion. N4 sigue sin marcar: faltan
conservación durante consultas, guardián, acciones y aceptación física. La GUI
sigue usando Solaar/Python. [Evidencia y próximos pasos](docs/native-events.md).


## Continuación N4: informes intercalados (20 de octubre de 2026)

Eliminado el vaciado silencioso de la cola en getters N2/N3 y emparejamientos.
La sesión conserva hasta 128 informes entre consultas; el desbordamiento cierra
el transporte y nunca se convierte en snapshot parcial válido. Los helpers internos
`apply_changes_session` y `apply_button_changes_session` conservan el propietario
y la cola durante validación, setter y confirmación; las CLI N3 siguen aisladas.
El observador procesa el backlog inicial y lo distingue con `queued_at_start` y
`queued_before_observation`; sus tiempos son de procesamiento, no de llegada física.

**335 pruebas aprobadas**, incluidas 124 Rust (75 biblioteca, 23 host, 26 integración).
Siete regresiones sintéticas nuevas cubren intercalado, orden y saturación antes y
después del ACK, sin reenvíos ni afirmaciones de rollback. Frontend, build, check de
todos los targets y suite completa pasan. Las pruebas de sockets Unix requieren
salir de la restricción del sandbox; la suite regular no accede al mouse.
No hay nueva aceptación física. Siguen pendientes guardián, consumo durante
consultas, cancelación entre getters y acciones antes de activar diversion.
[Evidencia y límites](docs/native-events.md).


## Continuación N4: entrega durante consultas (20 de octubre de 2026)

`Session` admite `ReportConsumer` síncrono y único, instalado tras validar identidad
y capacidades. Entrega primero la FIFO y luego cada informe no correlacionado durante
consultas, sin un segundo lector ni acumularlo en la cola. ACK/errores correlacionados
permanecen separados. `poll_consumer` sirve para reposo; `receive` rechaza competir
con el consumidor. Los requests conservan su plazo absoluto y una sola escritura,
con esperas/checkpoints de hasta 50 ms para cancelación y salud.

Errores y panics recuperables del consumidor cierran el transporte, sin reenvío.
Los setters siguen informando `attempted` si el fallo ocurre después de enviar el
cambio. El observador pasivo usa la misma vía para FIFO/reposo y conserva sus límites;
su cancelador debe ser un callback propio `FnMut + 'static`. El setup inicial aún
no tiene consumidor. No hay activación de reporting ni recuperación por guardián.

**347 pruebas aprobadas**: 136 Rust (78 biblioteca, 23 host, 35 integración) y
211 Python/TypeScript. Doce regresiones nuevas cubren entrega durante diagnóstico,
setters, errores, panics, cancelación, 300 informes intercalados y plazos. Build,
check de todos los targets y suite completa pasan; sockets Unix locales requieren
salir de la restricción del sandbox. Sin nueva aceptación física.
Siguiente: propietario/guardián, acciones y aceptación antes de activar diversion.
[Contrato y límites](docs/native-events.md).


## Continuación N4: propietario pasivo persistente (20 de octubre de 2026)

`native/owner.rs` incorpora `PassiveEventOwner`: una sesión privada con identidad,
snapshot validado, consumidor, poll, refresh y cierre. Reutiliza el motor de captura
del observador, con presupuesto cooperativo de hasta 30 s y límites existentes.
Detecta cambios de identidad, feature, catálogo, capacidades, mapping o reporting;
cierra y conserva el último snapshot válido. Stop/Drop sueltan el descriptor sin E/S,
reapertura o setters. Los eventos de consultas llevan `during_query:true`.

Diez regresiones nuevas elevan la suite a **357 pruebas aprobadas**, incluidas
146 Rust (88 biblioteca, 23 host, 35 integración). Frontend, build, check de todos
los targets, suite completa y formato pasan. Pruebas regulares sin mouse.

Comprobación real con `native_events --seconds 5 --refresh`: misma identidad,
una segunda lectura válida, 5001 ms de escucha, cero informes/eventos, cierre al
plazo y perfiles con hash idéntico. No habilita diversion ni verifica captura activa.
La evidencia de fallos sigue siendo sintética. Guardián, recuperación independiente,
servicio de acciones e integración GUI permanecen pendientes.
[Contrato, API y evidencia](docs/native-owner.md).


## Continuación N4: cancelación durante inicialización (20 de octubre de 2026)

`PassiveEventOwner::open` conserva el callback durante el diagnóstico inicial.
Una sesión con cancelación de inicialización comprueba antes de E/S y entre
informes, con esperas de lectura de hasta 50 ms, sin repetir consultas ni extender
sus plazos absolutos. Los informes no correlacionados siguen en la FIFO de 128;
solo se interpretan después de validar identidad y capacidades. Al instalar el
consumidor, este asume la cancelación con el mismo callback. Cancelar o un panic
recuperable del callback cierra el transporte y devuelve error sin snapshot válido.
El callback debe retornar pronto; no hay garantía de tiempo real ante syscalls
retrasadas ni cancelación autónoma sin ejecutar el propietario.

Cuatro regresiones sintéticas cubren cancelación/panic antes de E/S, interrupción
en consultas de identidad/catálogo/reporting, cancelación en espera silenciosa y
continuidad sin reenvíos ni pérdida de FIFO. **361 pruebas aprobadas**, incluidas
150 Rust (92 biblioteca, 23 host, 35 integración). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`, suite
Rust completa y formato. Los sockets Unix de las pruebas del host requieren salir
del sandbox. Sin pruebas físicas ni cambios en hardware/configuración.
Guardián independiente, recuperación, acciones y captura activa siguen pendientes.
[Contrato actualizado](docs/native-owner.md).


## Continuación N4: motor de restauración de reporting (20 de octubre de 2026)

`native/recovery.rs` añade `ReportingRecovery`: registro opaco tras getters reales
con reporting cero y restauración de un solo uso para CIDs 83/86/195. Revalida
identidad/feature/capacidades antes de escribir y limpia solo diversion temporal,
además de RAW_XY para 195, preservando mapping actual y flags ajenos. Exige ACK,
lectura posterior y confirmación final del conjunto. No-op no envía setters.
Plazo global de hasta 20 s, consumidor fijo sin acciones para descartar/contar hasta
4096 informes intercalados y cierre antes de devolver evidencia. Errores conservan
confirmaciones e intentos inciertos sin reabrir ni reenviar; detienen el lote.

Quince regresiones sintéticas elevan la suite a **376 pruebas aprobadas**:
165 Rust (107 biblioteca, 23 host, 35 integración) y 211 Python/TypeScript.
`pnpm test/build`, check de todos los targets, suite Rust completa, formato y diff
aprobados; las pruebas de sockets locales del host requieren salir del sandbox.
Sin acceso al mouse, cambios de configuración ni nueva aceptación física.

Esta pieza todavía no es el guardián ni protege por sí sola ante crash: faltan
proceso independiente, registro con ACK, lease heredado y handoff exclusivo del
lector antes de recuperación. La API no habilita diversion, no ejecuta acciones,
no se conecta a la GUI/CLI y no escribe perfiles. Esos pasos y la aceptación
física siguen pendientes antes de captura activa. [Contrato, límites y evidencia](docs/native-recovery.md).


## Continuación N4: guardián nativo con propietario exclusivo (20 de octubre de 2026)

`native/guardian.rs` conecta propietario pasivo y restauración en un proceso
independiente. El cliente solo recibe IPC de estado/refresh/cierre. El guardián
abre y conserva hidraw y lease desde preparación hasta recuperación; no hay
HID duplicado ni transferencia de lector entre procesos. El préstamo de la sesión
activa termina antes de restaurar sobre el mismo transporte original. Se confirma
registro e identidad antes de `ready`; el canal Unix anónimo valida padre/usuario,
secuencia, catálogo cerrado y tramas/plazos. El guardián tiene sesión de proceso
propia. EOF/cierre/fallo/panic cancelan al propietario, preservan evidencia y dan
paso a restauración; el cliente nunca mata al recuperador por timeout o Drop.

Quince casos nuevos (diez escenarios de procesos y cinco de config/IPC), junto
con dos entradas auxiliares del harness, elevan la suite a **393 aprobadas**:
182 Rust (124 biblioteca, 23 host, 35 integración) y 211 Python/TypeScript.
SIGKILL del cliente en reposo y durante un getter, lease retenido al recuperar,
fallos parciales, mensajes inválidos, cierre anticipado del socket y presión de
escritura tienen evidencia de procesos reales y mouse sintético. No hardware.
Pasan frontend, build, check de todos los targets, suite Rust completa y formato;
los sockets Unix locales requieren salir del sandbox.

La nueva CLI `native_guardian` es optativa, finita y no se ejecutó contra el mouse.
Puede limpiar los flags temporales registrados al cerrar, pero no activa ninguno,
no produce teclado, no carga perfiles y no conecta la GUI. N4 sigue parcial: faltan
Wayland/acciones/gestos nativos, confirmación de activaciones y aceptación física.
Se sustituyó el handoff/herencia HID previsto por propiedad continua en el hijo,
para eliminar lectores competidores al recuperar. El lease se adquiere en el
hijo y se conserva hasta terminar; no se hereda ni modifica su formato compartido.
[Contrato, pruebas, límites y próximos pasos](docs/native-guardian.md).


## Continuación N4: teclado Wayland nativo y limpieza (20 de octubre de 2026)

`native/keyboard.rs` añade salida Rust tipada enlazada estáticamente con C propio:
Alt/Ctrl mantenidos, edición, multimedia y atajos canónicos (63 teclas, 16 máscaras).
Handle exclusivo del mismo hilo, validación antes de emitir, cierre tras fallo,
liberación pendiente e idempotencia. C mantiene la ABI Python y añade cierre con
resultado; `native/wayland_wait.c` acota sincronización sin lectores competidores.
Estado y pulsación comparten plazos absolutos de 2 s por operación; cierre dispone
de su propio presupuesto. No se garantiza tiempo real de conexión/syscalls.

El guardián admite `with_keyboard_check` / `--keyboard-check`, optativo: solo crea
el teclado en estado cero y comprueba su cierre. Ready exige ese recurso cuando
se solicita. Al terminar, cierra propietario, libera teclado y restaura reporting
con lease retenido, incluso si falla la confirmación de liberación. El IPC sigue
cerrado a acciones/setters. No conecta acciones a botones ni habilita diversion.

19 casos nuevos: ocho Rust de teclado, cuatro de procesos/limpieza, seis Python
con el sincronizador C real y peer Wayland core simulado, y uno del cierre C.
**412 aprobadas**: 177 Python, 41 TypeScript y 194 Rust (136 biblioteca, 23 host,
35 integración). Pasan frontend, build, check de todos los targets, suite Rust,
formato y diff. Sockets privados fuera del sandbox; sin escritorio/mouse reales.

El build genera el archivo estático Rust y conserva la publicación atómica del .so
para el backend de transición. No hay Python ni dlopen en la nueva salida nativa.
Faltan consumidor de acciones/flancos/gestos, activación confirmada y aceptación
física antes de captura activa. N4 sigue parcial; la GUI conserva Solaar/Python.
[Contrato, plazos y evidencia](docs/native-keyboard.md).


## Continuación N4: motor Rust de acciones y gestos (20 de octubre de 2026)

`native/actions.rs` añade `ActionBindings` y `NativeActions`, independientes de
Vue, Solaar, perfiles y E/S HID. El parser admite el subconjunto cerrado de
teclado para CIDs 83/86/195. El motor posee un teclado Wayland, confirma cero al
recibirlo, sincroniza la primera lista sin acciones y suprime botones mantenidos
hasta soltarlos. Atajos por flanco, Alt/Ctrl compartidos, orden estable y cambio
de asignaciones sin pulsaciones artificiales. Cancelación libera modificadores,
descarta gestos y exige nueva sincronización; fallo de salida cierra sin reintento.

Gestos con dirección bloqueada, dominancia 6:5, pico conservado, límite de diez
segundos y acumulación saturada. Distancia física por techo entero con DPI real;
cambio/pérdida de DPI cancela el gesto físico sin reanudarlo al recuperar lectura.
Feedback distingue reconocimiento/ambigüedad; no acredita entrega de la acción.

23 pruebas nuevas elevan la suite a **435 aprobadas**: 177 Python, 41 TypeScript,
217 Rust (159 biblioteca, 23 host, 35 integración). Incluyen frames 1B04 sintéticos
pasados por el decodificador real y fallos de salida/limpieza. Se corrigió una
carrera previa de la prueba IPC de EOF: espera acotada ante descriptores retenidos
entre fork/exec de otros fixtures, sin cambiar el canal de producción.
Pasan `pnpm test`, `pnpm build`, check Rust de todos los targets, suite Rust
completa, formato y diff. Sin mouse ni compositor reales, sin cambiar configuración.

El motor aún no está conectado al consumidor vivo del guardián, la GUI ni perfiles.
El observador y el IPC permanecen pasivos; no se reproducen logs como acciones ni
se activa diversion. Siguiente: conexión síncrona al único consumidor, validación
de capacidades/raw XY, registro y confirmación de activaciones, salida de escritorio
y aceptación física. N4 sigue parcial; N5/N6 permanecen pendientes.
[Contrato, evidencia y límites](docs/native-actions.md).


## Continuación N4: consumidor tipado conectado al motor (20 de octubre de 2026)

`events::Event` separa la entrega directa de la evidencia JSON. `EventSink` y
`SinkHandle` conectan el motor al único consumidor de sesión, con resultado de
cierre compartido y estable. La FIFO del setup solo se registra; el primer estado
vivo sincroniza sin acciones. No se reproduce historial. Getters y reposo conservan
la misma entrega, y error/panic/saturación/cancelación cierran la salida.

`open_with_sink` es interno. Valida capacidades de asignaciones antes de adjuntar;
raw XY ambiguo cancela el gesto. Antes del refresh se invalida DPI: los gestos
físicos actuales/nuevos se suprimen hasta soltar; laterales y counts continúan.
Solo tras validar identidad/configuración se acepta el snapshot/DPI actualizado.
El guardián recoge el mismo cierre, destruye la salida y restaura con lease retenido.
Los errores de salida se conservan en el informe terminal.

`--keyboard-check` ahora prepara ese motor con asignaciones vacías; continúa sin
pulsaciones ni diversion. La CLI y el IPC no admiten asignaciones activas. Los
fixtures de proceso inyectan acciones con mouse/salida simulados para verificar
teclas mantenidas, flancos, gestos y cierre antes de restaurar, también tras
SIGKILL del cliente, panic, error, plazo y protocolo inválido.

17 casos nuevos (13 de sesión/motor y cuatro de procesos) elevan la suite a
**452 aprobadas**: 177 Python, 41 TypeScript y 234 Rust (176 biblioteca, 23 host,
35 integración). Pasan frontend, build, check Rust offline de todos los targets,
suite Rust completa, formato y diff. Pruebas con sockets privados fuera del sandbox,
sin mouse/compositor reales ni cambios de configuración.

Siguiente: configuración explícita validada de asignaciones, registro y setters
de activación diversion/raw XY con ACK y lectura sin caché, con consumidor y
recuperación preparados antes de escribir. Verificar intercalados durante esos
ACK, salida de escritorio y aceptación física. N4 sigue parcial; la GUI conserva
Solaar/Python, N5/N6 pendientes. [Contrato y límites](docs/native-action-delivery.md).


## Continuación N4: captura optativa confirmada (21 de octubre de 2026)

`native/activation.rs` implementa setters temporales fijos `0x03`/`0x33`, con
registro de recuperación opaco, consumidor y teclado preparados antes de escribir.
Cada CID exige una consulta completa actual de identidad/layout/capacidades y
una lectura inmediata de reporting/mapping. ACK exacto, lectura posterior y
consulta final confirman el conjunto. Conserva mapping, no toca flags persistentes
ni reintenta escrituras inciertas; el estado parcial queda en `activation`.

La salida permanece bloqueada durante activación, aunque el consumidor sigue
registrando los eventos intercalados. Solo una lista recibida después del último
setter puede sembrar sincronización; el historial del setup no genera flancos.
Sin esa lista, el primer estado posterior sincroniza sin acciones. Ready exige
todo confirmado, salida armada y asignaciones idénticas a las solicitadas.

`GuardianConfig::with_capture_actions` exige CIDs asignados exactamente iguales a
los registrados. La CLI separa `--capture --bindings-json` de `--observe` y del
keyboard-check vacío; JSON inicial acotado a 8192 bytes, validado también en el
hijo. Asignaciones fijas al arrancar, IPC limitado a estado/refresh/cierre. Espera
de arranque de captura: 65 s. Activación hasta 20 s dentro del intervalo de 1–30 s;
no se extiende observación para compensar activación lenta ni se activa en la GUI.

Cualquier fallo cierra motor/lector y permite al guardián restaurar los CIDs
registrados con lease retenido, también tras un setter incierto o SIGKILL del
cliente durante activación. Cambios de identidad/layout o dispositivo ausente
pueden impedir confirmar restauración; se conserva el error y no se escribe sobre
otra identidad. Flags de evidencia distinguen confirmación de posible escritura.

34 casos nuevos: 21 de activación, cinco de procesos, dos de configuración y seis
del parser CLI. **486 aprobadas**: 177 Python, 41 TypeScript y 268 Rust (204
biblioteca, 23 host, 35 integración, seis CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml --all-targets`, formato
y diff. Los sockets privados se prueban fuera del sandbox; no se abrió mouse ni
compositor reales.

N4 sigue parcial: falta aceptación física optativa de captura/acciones/recuperación
y salida nativa de escritorio. Después, servicio/perfiles/Tauri de N5 y retirada
de Solaar/Python en N6. No presentar fixtures como aceptación Bolt. La GUI
conserva su backend anterior. [Contrato, formato y límites](docs/native-capture.md).

## Continuación N4: escritorio nativo con preflight (21 de octubre de 2026)

`native/desktop.rs` añade las cuatro acciones Hyprland existentes con enum cerrado
y payloads Lua constantes. `ActionBindings` las admite en laterales y gestos;
el motor distingue salida de teclado/escritorio, sin convertir dispatchers en
atajos ni cambiar modificadores mantenidos. La frontera de teclado sigue
rechazando acciones de escritorio. No se aceptan shell ni dispatchers arbitrarios.

El motor exige preflight de versión/capacidades Lua antes de activar captura,
al aceptar snapshots y al reemplazar asignaciones. Cada acción vuelve a consultar
versión/capacidades, resuelve la sesión actual una vez y usa ese destino en sus
tres intercambios. Unix no bloqueante/CLOEXEC, peer del mismo usuario, plazo
absoluto de 1 s por intercambio, respuestas acotadas y EOF obligatorio. Una
confirmación incierta consume la acción, cierra salida y pasa a restauración,
sin reintento. La espera de arranque con escritorio es de 75 s para incluir
preflights; no amplía plazos de HID ni sesión. Solo teclado conserva 65 s.

29 casos nuevos: 16 de protocolo/socket, diez de motor/enrutamiento y tres de
procesos del guardián. Prueban preflight sin dispatch, versiones/rutas inválidas,
límites, fragmentación, goteo y timeout absoluto, flancos/gestos sin repetición,
modificadores conservados, falta de escritorio antes de setters y restauración
después de un dispatcher incierto. Todo el hardware y compositor son simulados.

**515 aprobadas**: 177 Python, 41 TypeScript y 297 Rust (233 biblioteca, 23 host,
35 integración y seis CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml --all-targets`, formato
y diff. Sockets/procesos privados fuera del sandbox; sin acciones en el escritorio
real ni cambios en configuración, servicios o permisos.

N4 sigue parcial por aceptación física optativa de captura/acciones/cancelación/
cierre y recuperación. Siguiente implementación: servicio nativo de perfiles y
reconciliación de N5, con contratos/schemas 1–10 y datos conservados, antes de
sustituir el worker Tauri. La GUI continúa con Solaar/Python; N6 retira esas
dependencias tras paridad y aceptación. [Contrato y evidencia](docs/native-desktop.md).

## Inicio N5: núcleo de perfiles y persistencia (21 de octubre de 2026)

`src-tauri/src/profiles/` separa el modelo/gestión local del backend HID y Tauri.
Valida documentos 1–10, tipos, catálogo de acciones, referencias y límites sin
hardware. Conserva campos extra, identidades y nombres existentes sin migrar al
leer. JSON mantiene precisión de metadatos numéricos con `arbitrary_precision`
en serde_json; no se añade una dependencia de runtime. Los valores creados en
Rust también deben poder releerse dentro del límite normal de anidación.

Gestión local: rename/delete/export/import_preview/import/rule_set/rule_delete
y auto. Importar retargetea al dispositivo seleccionado, resuelve colisiones y
no instala activaciones/reglas externas. Renombrar/eliminar actualiza referencias
solo del mismo dispositivo. La selección por foco es pura y exige perfil general
armado; todavía no consulta compositor ni reconcilia hardware.

`ProfileStore` conserva la ruta `linux-scrollmaster/profiles.json`, lectura sin efectos y revisión
opaca con bytes/directorio. Publicación atómica 0600, lock cooperativo nativo,
fsync y lectura posterior; detecta cambios antiguos, enlaces/permisos inseguros y
sustitución de directorio. Un fallo posterior al rename indica publicación sin
confirmar; no se simula rollback ni se aplica hardware. La API de gestión local
persiste cambios antes de devolver éxito; preview/export/noop no reescriben.
El Python actual no comparte ese lock: no conectar ambos escritores al mismo
archivo. El servicio futuro deberá sustituir al anterior, sin ejecución paralela.

`native_profiles --validate/--manage` ofrece validación y cálculo offline por
stdin/stdout, con tamaño acotado; no acepta rutas ni escribe archivos o hardware.
La biblioteca de persistencia solo se ejercitó en directorios temporales.

41 pruebas nuevas: 35 biblioteca, cinco CLI y una Python. Los fixtures contienen
21 documentos y cuatro operaciones escritos a mano, consumidos por ambos núcleos
de forma independiente. Se cubren schemas, tipos, nombres/Unicode, metadatos y
precisión, reglas, importaciones sin activación, revisión antigua/concurrente,
permisos/enlaces, directorios sustituidos y fallos antes/después de publicación.

**556 aprobadas**: 178 Python, 41 TypeScript y 337 Rust (268 biblioteca, 23 host,
35 integración y once CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml --all-targets`, formato
y diff. Pruebas sin mouse/compositor ni perfiles reales; no se alteró configuración.

Siguiente: servicio/reconciliación nativos y guardado desde snapshot completo;
validar el perfil completo antes de cualquier escritura, persistir desarmado antes
de edición/restauración, preservar fallos parciales y reconexión. Después conectar
el contrato Tauri con un único worker y propietario. N5 sigue parcial, N4 requiere
aceptación física optativa y N6 retirará Solaar/Python tras paridad. La GUI mantiene
el backend anterior. [Contrato, uso offline y límites](docs/native-profiles.md).

## Continuación N5: servicio y reconciliación síncronos (21 de octubre de 2026)

`service.rs` orquesta operaciones locales, guardado desde descripción completa,
ediciones/restauración y reconciliación por petición. `Changes` valida el conjunto
de forma estructural; `NativeProfileBackend` comprueba ajustes y remapeos juntos
antes del primer setter, conserva la misma sesión y confirma todo al final.
Los valores ya presentes se leen y no reenvían setters. Las asignaciones de
software fallan antes de getters/setters hasta integrar su ciclo de vida del guardián.

Set/remap/restore exigen preflight y revisión actual, desarman mediante publicación
confirmada antes de hardware y conservan ese desarmado ante un fallo parcial.
Un conflicto/fallo de persistencia impide escribir. Snapshot reconcilia solo perfiles
armados de la identidad correcta, aplicando reglas para el foco suministrado y
perfil general cuando no coincide. No conserva éxito en caché. Los errores retienen
confirmaciones históricas/objetivo incierto; una aplicación automática fallida
retira datos potencialmente obsoletos y no anuncia reconciliación.

Save exige seis valores actuales disponibles y un catálogo de botones válido;
preserva metadatos y no elimina controles previos omitidos por una lectura parcial.
No arma perfiles nuevos. Todo comparte el presupuesto de petición al interactuar
con el backend. Todavía no hay descubrimiento/reapertura automática, lectura de
foco/catálogo de aplicaciones, worker/IPC Tauri ni activación de software mediante
este adaptador. El dueño exterior deberá conservar lease/exclusión durante la sesión.
La GUI y su escritor Python permanecen separados de esta biblioteca experimental.

29 pruebas nuevas: 19 de servicio con archivos temporales y diez de backend con
frames HID++ escritos a mano. Cubren orden del desarmado, persistencia concurrente,
aislamiento/foco, ausencia de caché, guardado incompleto, plazos/recibos incorrectos,
fallos parciales, preflight conjunto, ACK sin efecto y deriva de DPI tras remapeo.
Un caso recorre perfil persistido → servicio → backend Rust → protocolo simulado,
y confirma que el segundo poll no repite setters. No es evidencia física.

**585 aprobadas**: 178 Python, 41 TypeScript y 366 Rust (297 biblioteca, 23 host,
35 integración y once CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml --all-targets`, formato
y diff. Sin mouse/compositor/perfiles reales ni cambios en configuración del usuario.

Se incorporó a `AGENTS.md` la regla del usuario de máximo 1000 líneas por archivo
de código, con excepciones justificadas. Auditoría de 111 archivos propios de
Rust/Vue/TS/JS/Python/C/CSS/HTML/shell: ninguno supera 1000; el mayor tiene 977.
Los nuevos módulos/pruebas se separaron por responsabilidad; no se necesitó excepción.

Siguiente: integrar guardián/salidas para perfiles de software y dueño/worker
nativo con arranque offline, descubrimiento/reconexión y lease; después adaptar
y sustituir el contrato Tauri con un solo propietario del mouse y archivo.
N5 sigue parcial; N4 requiere aceptación física y N6 retirará Solaar/Python tras
paridad e instalación limpia. [Contrato y límites](docs/native-service.md).

## Continuación N5: dueño con conexión diferida (21 de octubre de 2026)

`NativeProfileOwner` implementa `ProfileBackend` para el servicio ya existente.
Construcción offline con slot/Unit ID explícitos, sin descubrimiento, archivos
ni lease. La gestión local no lo llama. Al necesitar hardware, excluye procesos
competidores, toma el lease compartido, descubre una interfaz Bolt sin ambigüedad
y valida identidad/catálogo mediante una lectura completa. Reutiliza la sesión
mientras responde; cualquier error la destruye antes de liberar el lease. Una
petición posterior puede descubrir de nuevo, manteniendo Unit ID y slot.

Apply exige preparación del mismo conjunto/sesión/plazo, consumible una vez.
Nunca reconecta entre preflight, desarmado persistido y setters. Una escritura
incierta conserva confirmaciones/objetivo, cierra y no se reintenta internamente.
Una lectura posterior establece estado actual. Software se rechaza antes de
adquirir lease o consultar. No hay diversion, teclado, hilo, proceso o timer nuevo.
El worker Tauri sigue usando Solaar/Python. Varios receptores, cambio de slot,
aislamiento de syscalls bloqueantes y contrato completo del worker quedan pendientes.

21 pruebas nuevas (18 de política/servicio y tres con backend Rust y protocolo
HID++ sintético) cubren offline, reconexión, identidad, conflicto de controlador,
plazos, preparación, cierre/lease, recibos incorrectos y fallos parciales. No se
abrió mouse, compositor ni perfiles reales. **606 aprobadas**: 178 Python,
41 TypeScript y 387 Rust (318 biblioteca, 23 host, 35 integración y once CLI).
Pasan pnpm test/build, cargo check/test offline con --all-targets, formato y diff.
Se revisaron 115 archivos de código propios: ninguno supera 1000 líneas; máximo
977, sin excepciones. [Contrato, evidencia y límites](docs/native-profile-owner.md).

Siguiente: integrar guardián/salidas para perfiles con software y worker/IPC nativo;
después adaptar y sustituir el worker Tauri con un solo propietario de HID y
perfiles. N5 sigue parcial; reconexión/captura/recuperación requieren aceptación
física optativa y N6 retirará Solaar/Python tras paridad e instalación limpia.

## Continuación N5: worker JSON-lines y supervisor (21 de octubre de 2026)

`scrollmaster-native-worker` aloja el servicio Rust y su dueño en un proceso
separado. CLI explícita con slot/Unit ID, framing de entrada 256 KiB más LF,
respuesta de hasta 2 MiB y operaciones/campos cerrados. Conserva perfiles, estado
y sesión entre peticiones, sin respuestas espontáneas. EOF/shutdown/fallo de
canal destruyen el servicio; shutdown confirma después de cerrar. Las tramas
incompletas o grandes son terminales y no ejecutan datos posteriores.

`bridge.rs` pasa a biblioteca compartida. `Worker::spawn_native` permite lanzamiento
explícito desde Rust; la GUI sigue iniciando Python. El supervisor usa pipe de
entrada no bloqueante, poll y un solo plazo para enviar/recibir, evitando que un
hijo que no lee bloquee antes del timeout. Respuestas pendientes acotadas;
errores terminales hacen kill/wait y no permiten reutilizar ni repetir el intento.
Un error de servicio válido mantiene el proceso. Cargo conserva la GUI como
default-run; empaquetado del segundo binario e integración final pendientes.

24 pruebas nuevas: doce de frontera/servicio, seis de supervisor y seis con el
ejecutable Rust real y perfiles temporales. Las tres antiguas de supervisor se
movieron del host a la biblioteca. No hubo lectura/aplicación de hardware, foco
real ni perfiles del usuario. **630 aprobadas**: 178 Python, 41 TypeScript y
411 Rust (339 biblioteca, 20 host, 41 integración y once CLI). Pasan pnpm test/build,
cargo check/test offline --all-targets, formato y diff. Auditoría de 119 archivos
de código propios: ninguno supera 1000 líneas; máximo 977, sin excepción nueva.
[Contrato, evidencia y límites](docs/native-worker.md).

Siguiente: foco/catálogo real y guardián/salidas para perfiles con software; cerrar
paridad del contrato y sustituir el worker Tauri con un solo propietario de HID
y perfiles. N5 sigue parcial, aceptación física optativa pendiente y N6 retirará
Solaar/Python tras paridad e instalación limpia.

## Continuación N5: foco y catálogo nativos (21 de octubre de 2026)

`native/focus.rs` reutiliza el transporte de escritorio con dos peticiones fijas
de solo lectura: j/activewindow y j/clients. Resuelve la sesión una vez por petición,
valida credenciales del peer y comparte un presupuesto total de 1 s; cada respuesta
admite 1 MiB. Solo conserva clases válidas, sin títulos, PID ni direcciones. No
requiere versión/Lua ni invoca dispatchers. Un error retira datos parciales y
el siguiente sondeo consulta de nuevo, sin caché de foco ni búsqueda de sesiones.

El worker consulta foco en cada snapshot y lo entrega al servicio, que ya elige
el perfil por coincidencia exacta si hay general armado. Sin coincidencia/foco,
o ante error, usa el general. Una edición manual sigue desarmando el general y
las reglas antes de escribir. Applications ya devuelve catálogo/perfiles incluso
offline, sin abrir HID ni reconciliar. La UI no puede suministrar foco, comandos
ni rutas; las demás operaciones locales no consultan el compositor. No hay
polling/socket2 nuevo: el llamador debe seguir enviando snapshots.

24 pruebas nuevas: doce del adaptador/sockets, nueve de worker/servicio con
dispositivo sintético y tres del ejecutable con compositor simulado. Cubren
privacidad, validación, plazos, cambio/fallo de sesión, reglas/fallback/manual,
catálogo sin hardware y recuperación. No se abrió compositor/mouse ni perfiles
reales. **654 aprobadas**: 178 Python, 41 TypeScript y 435 Rust (360 biblioteca,
20 host, 44 integración y once CLI). Pasan pnpm test/build, cargo check/test offline
--all-targets, formato y diff. Auditoría de 123 archivos de código propios:
ninguno supera 1000 líneas; máximo 977, sin excepciones nuevas.
[Contrato, evidencia y límites](docs/native-focus.md).

Siguiente: integrar perfiles con acciones/gestos y ciclo del guardián; completar
paridad de respuestas y sustituir worker Tauri con un solo propietario de HID y
perfiles. N5 sigue parcial y la GUI conserva Solaar/Python. Aceptación física
optativa pendiente; N6 retira dependencias y prueba instalación limpia.


## Continuación N5: botones de perfiles y guardián finito (21 de octubre de 2026)

`ProfileButtonPlan` compila el objeto buttons usando el contrato persistido,
conserva unidades/atajos y separa destinos nativos de acciones de software.
El guardián finito acepta el mapa mediante `--profile-buttons-json`: registra
solo los controles software, exige los destinos nativos ya coincidentes en su
sesión fresca y rechaza divergencias antes de diversion. La activación existente
revalida el catálogo entre setters y restaura ante cambios parciales; ready
incluye el mapa original, contrastado por el padre. No aplica ajustes/remapeos,
no carga/arma perfiles ni conecta el servicio duradero.

14 pruebas nuevas: cinco de compilación/contrato, dos de configuración, cuatro
con procesos y mouse HID++ sintético, y tres de CLI. Cubren mapas mixtos,
rechazo sin captura, cambio nativo durante activación, acciones solo de software,
restauración y ready sin requisitos nativos. No se abrió hardware, compositor ni
perfiles reales. **668 aprobadas**: 178 Python, 41 TypeScript y 449 Rust (371
biblioteca, 20 host, 44 integración y catorce CLI). Pasan pnpm test/build,
cargo check/test offline --all-targets, formato y diff. Los sockets sintéticos
se probaron fuera del sandbox tras errores de permisos dentro. Auditoría de
128 archivos propios: ninguno supera 1000 líneas; máximo 971. Configuración del
guardián y de fixtures separadas por responsabilidad, sin nuevas excepciones.
[Contrato, evidencia y límites](docs/native-profile-actions.md).

Siguiente: integrar propietario/guardián duradero con servicio, aplicación conjunta
de ajustes/remapeos y software, cambios por foco y reconexión. Después, paridad
del worker/Tauri y N6. El dueño actual sigue rechazando software antes de adquirir
hardware. La GUI conserva Solaar/Python; aceptación física nativa optativa pendiente.


## Continuación N5: propietario independiente de perfiles (21 de octubre de 2026)

El modo experimental `--serve-guarded` conecta `ProfileService` con
`ProfileGuardianOwner`: el padre conserva persistencia/perfiles y el hijo
independiente conserva descriptor/lector/lease. Se inicia solo en peticiones de
hardware; la gestión offline y el rechazo de software antes de adquirir se
mantienen. El guardián usa el dueño nativo existente para descubrimiento,
identidad, controles concurrentes y reconexión entre peticiones.

El RPC cerrado snapshot/prepare/apply/shutdown valida campos y secuencia. Los
plazos viajan como vencimientos absolutos monotónicos; el ticket de preflight se
consume una sola vez para el mismo lote/plazo, después del desarmado persistido.
No se reconecta ni repite un apply incierto. Timeout/fallo cierra IPC sin matar al
hijo; el mismo proxy espera su salida antes de otro propietario. Shutdown exige
cierre de backend y recogida del hijo antes de stopped; un timeout informa error
y permite continuar la limpieza. SIGKILL del cliente se probó con lease temporal.

25 entradas nuevas: 21 de biblioteca, incluidos dos harnesses de procesos, y cuatro
integraciones del ejecutable. Cubren lotes/tickets/plazos, errores parciales,
recibos incorrectos, lease durante cierre, muerte del cliente y rechazo de
shutdown exitoso mientras el hijo sigue cerrando. El guardián de producción también
pasó handshake/rechazo de software/shutdown con un canal privado real, sin getters.
**693 aprobadas**: 178 Python, 41 TypeScript y 474 Rust (392 biblioteca, 20 host,
48 integración y catorce CLI). Pasan pnpm test/build, cargo check/test offline
--all-targets, formato y diff. Los sockets sintéticos requieren ejecución fuera
del sandbox. Los 133 archivos propios cumplen el máximo de 1000 líneas; mayor
971, sin nuevas excepciones. No se abrió hardware, compositor ni perfiles reales.
[Contrato, evidencia y límites](docs/native-profile-guardian.md).

Siguiente: integrar consumidor de eventos, motor/salida y ReportingRecovery en
este propietario duradero, con cambios de perfil/foco, DPI y reconexión. El modo
nuevo solo admite ajustes/remapeos nativos; no habilita diversion. No se conectó
Tauri ni se retiró Solaar/Python. La aceptación física nativa continúa pendiente.


## Continuación N5: consumidor de perfiles duradero (21 de octubre de 2026)

El backend instala su único consumidor después de validar identidad/catálogo,
conserva eventos durante getters/setters y expone evidencia `native_events` en
snapshots. Registra hasta 64 eventos recientes con secuencia/fase y contadores de
historial retirado. Los informes previos se marcan initialization y nunca se
reproducen como acciones. El presupuesto de 4096 informes se renueva por operación,
no por vida de sesión; un error es terminal. Se detecta deriva de layout, sin
bloquear ajustes/remapeos por ausencia de capacidades de diversion.

El guardián alterna IPC y poll de hasta 50 ms en su mismo hilo/lector, sin adquirir
hardware offline y sin postergar órdenes bajo eventos continuos. La trama parcial
conserva un plazo absoluto. Un error idle cierra lector antes de lease, invalida
preparación y se entrega una vez en la siguiente petición, que no se ejecuta.
Una posterior puede reconectar. El conector guardado comprueba el peer antes de
E/S y entre informes, incluida inicialización; no se interrumpen syscalls ni se
promete cancelar un efecto ya en curso.

27 pruebas nuevas: ocho del consumidor, trece del backend/protocolo, dos del dueño
y cuatro de procesos/servidor. Cubren memoria y duración, presupuesto por operación,
FIFO antiguo, filtros/XY, consulta con 300 informes, capacidades solo nativas,
deriva de layout, EOF antes de getters/setters, error tras remapeo sin reintento,
lease/tickets y equidad IPC. **720 aprobadas**: 178 Python, 41 TypeScript y 501 Rust
(419 biblioteca, 20 host, 48 integración y catorce CLI). Pasan pnpm test/build,
cargo check/test offline --all-targets, formato y diff. Sockets sintéticos fuera
del sandbox tras errores de permisos dentro. Los 137 archivos propios cumplen
1000 líneas como máximo; mayor 971, sin excepciones nuevas. No se abrió hardware,
compositor ni perfiles reales. [Contrato y evidencia](docs/native-profile-events.md).

Siguiente: salida preparada y motor con ReportingRecovery dentro del propietario
independiente, antes de activar software. Después, transiciones de perfiles/foco
con DPI, reconexión de captura y paridad Tauri/N6. No se habilitó diversion ni se
cambió el backend activo de la GUI. La aceptación física nativa sigue pendiente.

## Continuación N5: captura residente optativa (21 de octubre de 2026)

`--serve-guarded-capture` conecta `GuardedProfileBackend`, motor/salida preparada,
activación y ReportingRecovery en el dueño independiente. Los modos previos
mantienen rechazo de software antes de adquirir hardware. Preflight valida todo
el lote y abre salida cero antes del desarmado persistido. Apply consume el mismo
ticket/lote/plazo, revalida y confirma escrituras sin repetir efectos inciertos.
No se arma salida hasta verificar valores nativos y reporting del lote completo;
el FIFO anterior no genera acciones. El consumidor sigue siendo único y residente.

El contenedor privado retiene el descriptor si se invalida la sesión. Se cierra
salida y sesión activa antes de transferirlo a recuperación, sin duplicar/reabrir
hidraw. La restauración conserva mappings/flags ajenos y mantiene lease hasta
terminar. El dueño ahora ejecuta close bajo lease también ante error/Drop. El
servidor no responde stopped si close falla; errores de recuperación son
persistentes e impiden reconectar ese dueño. Una captura con apply/IPC incierto
no se reinicia automáticamente por el proxy.

El plan de botones es fijo hasta cierre confirmado; mismo plan y ajustes escalares
sí están admitidos. Cambiarlo se rechaza antes de sus setters y restaura el anterior.
La UI/GUI conserva Python/Solaar. No se abrió hardware, compositor ni perfiles reales.
Las acciones están verificadas con transporte y salida simulados, no físicamente.

30 entradas nuevas: 28 de biblioteca, incluidos dos harnesses, y dos integraciones.
Cubren lotes mixtos, modificadores/gestos, FIFO/sincronización, memoria/tickets,
salida/escritorio fallidos, errores inciertos, cancelación, procesos/EOF/SIGKILL,
lease, cierre persistente y selección de modo. **750 aprobadas**: 178 Python,
41 TypeScript y 531 Rust (447 biblioteca, 20 host, 50 integración y catorce CLI).
Pasan pnpm test/build, cargo check/test offline --all-targets, formato y diff.
Pruebas de sockets fuera del sandbox tras fallos de permisos. Los archivos propios
de código conservan el máximo de 1000 líneas. [Contrato y evidencia](docs/native-profile-capture.md).

Siguiente: transiciones software/nativo y perfiles/foco, reconexión de captura
sin reintentos inciertos y paridad Tauri; después aceptación física optativa y N6.
La recuperación no garantiza muerte del guardián, pérdida de energía o dispositivo
inaccesible y no tiene journal persistente. N5 sigue parcial.

## Continuación N5: transiciones de plan en caliente (21 de octubre de 2026)

`--serve-guarded-capture` ya no rechaza un plan de botones distinto: el mismo
dueño desarma la entrega, cierra la salida anterior, limpia en vivo solo los bits
temporales registrados (`ReportingRecovery::release_live`, misma rutina que la
restauración pero sobre la sesión y consumidor residentes, sin reabrir hidraw) y
exige confirmación antes de escalares, remapeos nativos y nueva activación. La
salida de reemplazo se abre en preflight y se conecta bloqueada tras registrar
una recuperación fresca. Software→solo nativo deja la captura inactiva. Una
liberación incierta es terminal y no se repite; la limpieza final relee y
restaura con el descriptor retenido.

Rust: **535 aprobadas** (451 biblioteca, 20 host, 50 integración y 14 CLI), cinco
nuevas y una sustituida. Pasan pnpm test/build, cargo check/test offline
--all-targets, formato y diff; máximo 971 líneas por archivo de código. No se
abrió hardware, compositor ni perfiles reales.
[Contrato y evidencia](docs/native-profile-capture.md#transiciones-de-plan-bajo-el-mismo-dueño).

Siguiente: transiciones disparadas por foco/perfil desde el servicio duradero,
reconexión de captura sin reintentos inciertos y paridad Tauri; después
aceptación física optativa y N6. N5 sigue parcial.

## Cierre de implementación del Hito N (21 de octubre de 2026)

Petición: terminar todo lo pendiente del Hito N para poder hacer las pruebas.
Queda implementado y probado automáticamente; **la aceptación física no se ha
ejecutado** y no se atribuye a estas pruebas.

- Transiciones por foco de punta a punta (servicio → dueño → captura) y remap
  parcial con semántica por botón, como Python.
- Reconexión: tras limpieza confirmada, la siguiente petición reconecta; la fallida
  no se repite. Limpieza no confirmada o IPC desconocido siguen siendo terminales.
- GUI: `SCROLLMASTER_BACKEND=native` o feature `native-default`; localizador de
  solo lectura, el ejecutable de la GUI actúa también como worker/guardián,
  presentación compatible con Vue, gestión offline y cierre confirmado.
- La comprobación de controladores excluye la propia cadena GUI → worker → guardián;
  Solaar, logid, otras instancias y el worker Python siguen bloqueando.
- N6: `packaging/arch-native` sin Python/Solaar, `prepare.py --variant native` y
  regla udev `uaccess` solo para la interfaz 02 del receptor 046d:c548
  (verificada en sysfs y con `udevadm verify`, no instalada).
- Pendiente conocido: `gesture_feedback` nativo es `null` (sin vista previa de
  dirección en el editor de gestos).

- Corrección de la sesión HID++: el plazo de `poll_consumer` limita la espera; un
  informe ya retirado de la FIFO se entrega siempre. Antes, una entrega lenta (por
  carga o despacho de escritorio) cerraba la sesión y restauraba la captura; lo
  delataba una prueba intermitente de 5000 eventos (fallaba 1 de cada 4 veces).

**780 aprobadas**: 183 Python, 41 TypeScript y 556 Rust (470 biblioteca, 20 host,
52 integración y 14 CLI), estables en dos ejecuciones completas. Build release con
`--features native-default` completado. Pasan pnpm test/build, cargo check (también con
`--features native-default`) y test offline `--all-targets`, formato y diff.
Máximo 971 líneas por archivo de código. Sin hardware, compositor ni perfiles
reales. [Integración](docs/native-gui.md) y [protocolo de aceptación](docs/native-acceptance.md).

## Aceptación física guiada del Hito N (22 de octubre de 2026)

Pruebas en el equipo de referencia (receptor Bolt `hidraw7`, slot 2, `<UNIT_ID>`,
1B04 v5, batería 20 %), sin Solaar ni logid, sin root. Las ejecutó Claude con un
driver que mantiene vivo el worker real `--serve-guarded-capture` (perfiles en un
`XDG_CONFIG_HOME` temporal) y el usuario interactuó con el mouse cuando se le pidió.
El bloque 1 usó la GUI real (`SCROLLMASTER_BACKEND=native pnpm tauri dev`) con los
perfiles reales. Línea base y estado final por lectura directa: DPI 1800,
smooth-invert y smooth-resolution off, ratchet 2, smart-shift 10, thumb-invert on;
83/86/195 nativos con `flags` 0.

| Bloque | Resultado |
|---|---|
| 1 Arranque nativo | Localiza sin Solaar; batería, DPI y perfiles. Mouse apagado: aviso y perfiles visibles; renombrado offline correcto; vuelve al encender. «Reaplicar al reconectar» repone DPI 1850 tras apagar (el 3S vuelve a 1000 de fábrica: el DPI no persiste en el mouse) |
| 2 Ajustes/remap | Seis ajustes cambiados, confirmados y restaurados; smart-shift rechazado correctamente en rueda libre y aceptado en trinquete; 83→86→83 y 195→82→195 |
| 3 Captura | Atrás=Ctrl (zoom, sin Ctrl pegado), Adelante=Copiar (portapapeles verificado), gestos volumen/espacios, distancia estable con DPI 1000 |
| 4 Foco | Firefox=Alt, kitty=Ctrl (general), VSCode=Atrás nativo `flags` 0, alternando con un solo worker; remap de un botón conserva la captura del otro |
| 5 Recuperación | `kill -9` con Atrás mantenido: Ctrl no queda pegado, `flags` 0, reconexión; cierre confirmado con captura activa; apagar/encender y suspender: el siguiente sondeo reaplica el perfil activo |
| 6 Instalación limpia | Pendiente: requiere commit, `makepkg -si` y desinstalar Solaar |

Defectos encontrados en hardware y corregidos (con regresiones sintéticas):

- **Escaneo de procesos**: leer `/proc/<pid>/comm` de un proceso que termina da
  `ESRCH`, no `ENOENT`; bloqueaba la activación de la captura. `access::vanished`.
- **Sesión HID++**: un informe leído justo después del tramo de espera de 50 ms
  (no del plazo total) cerraba la sesión, en `poll_consumer` y en `exchange`. La
  captura se caía sola al usar los botones. El rechazo tras el plazo **total** se
  mantiene; los dobles `late_ack`/`late_setter_reply` ahora modelan ese plazo.
- **Primer raw XY**: el backend nativo no descartaba el informe anómalo inicial de
  1B04 v5 que el adaptador Python sí omitía. Medido en 16 gestos grabados: 6
  cambiaban de dirección (izquierda detectada como arriba, etc.). Se omite por
  pulsación en `Runtime::event`; los gestos exigen versión 1B04 verificada.
- **Reanudación**: la limpieza se ejecutó con el enlace del mouse caído (HID++ 1.0
  error 4) sin enviar ningún setter y el dueño quedaba terminal. `ReportingRecovery::
  restore_or_defer` devuelve el registro si `attempted` es nulo; el conector de
  captura lo completa (relee y solo borra bits propios) antes de otra conexión y
  `close` informa si sigue pendiente. Un setter dudoso sigue siendo terminal.
- **Sondeo residente**: cada tramo de 50 ms recorría `/proc` buscando controladores
  y, con carga (al reanudar), agotaba el tramo y desmontaba la captura. El sondeo ya
  no escanea; cada petición y conexión sí.

Resuelto después: el error HID++ 1.0 de «mouse sin enlace» (código 4) se muestra en
lenguaje normal (`presentation::unreachable_mouse`). Pendiente: bloque 6; luego
default nativo.
**780 aprobadas** tras los cambios: 183 Python, 41 TS, 556 Rust (482 biblioteca,
20 host, 54 integración). Máximo 905 líneas por archivo de código.

**Gestos de volumen continuos (22 de octubre de 2026).** El usuario pidió que el
volumen siga al movimiento mientras se mantiene el botón. `actions.rs`: volumen
arriba/abajo emiten un paso por umbral recorrido mientras se mantiene el botón, con
histéresis de un paso y retorno reversible; nada al soltar. Otras acciones no
cambian. Aceptado físicamente. 484 pruebas de biblioteca Rust.

**Gestos con respuesta por velocidad (22 de octubre de 2026).** El usuario eligió:
acción al iniciar el gesto, ponderación por velocidad y cambio de eje a mitad; no
la repetición de acciones no continuas. `actions.rs`: cada reporte suma
`d·(|d|·1000/DPI)^0,4` (a 1000 DPI es `|d|^1,4`; la distancia en mm
sigue estable con DPI); la acción corre al fijar el eje dominante (factor 1,2); el
otro eje toma el control si alcanza la distancia y domina el movimiento desde la
última acción; el volumen da un paso cada media distancia y reinicia el acumulado;
al soltar solo se dispara el clic. Pruebas reescritas con la nueva semántica
(incluida la estabilidad con DPI); 558 Rust estables en tres ejecuciones.
Aceptado físicamente por el usuario (acción al iniciar, velocidad y cambio de eje).

**Instalación limpia N6 y ajustes visuales (22 de octubre de 2026).** El usuario
instaló el paquete compilado (`pacman -U`) y desinstaló Solaar. Tras la recarga de
udev solo `hidraw7` (interfaz 02) tiene ACL del usuario; `hidraw5/6` ya no. La app
instalada arranca sin `SCROLLMASTER_BACKEND` con worker y guardián nativos desde
`/usr/bin` y sin procesos Python; el usuario repitió ajustes, captura Ctrl, gestos y
apagado/encendido: todo correcto. Pendiente: desinstalar y comprobar perfiles.

Observaciones de la prueba y correcciones: en 4K (escala 1,25 de Hyprland) la UI era
diminuta y ocupaba media pantalla; ahora escala por tramos de ancho (`zoom` 1,3/1,6/
1,85). La batería era pequeña y no indicaba carga: `presentation` expone
`battery_status` desde el byte de estado 1004 y la UI muestra icono con nivel, ⚡ al
cargar y aviso bajo 20 % (`src/battery.ts`, con pruebas). El texto de la interfaz ya
no se selecciona al arrastrar (campos y avisos siguen copiables). Verificado con
captura en el monitor 4K.

**Mouse 3D en la tarjeta del dispositivo (22 de octubre de 2026).** Descartados, por
no parecerse al mouse (usuario): el modo 3D de la skill Anatomy (solo piezas
convexas: carcasa en terrazas) y un modelo procedural propio. El modelo de Sketchfab
sugerido no es descargable ni libre. Solución actual: el usuario descargó en `3d/`
(ignorado por git) una carcasa CAD de MX Master 3 para impresión (`MxMaster3v58.step`)
y sus botones (`MouseButtonsMxM3v68.step`). `art/mouse-render/convert.mjs` los
tesela con `occt-import-js` (devDependency) a un caché local ignorado;
`step-extras.js` agrega lo que la carcasa impresa no trae, medido con rayos sobre la
malla: rueda y botón de modo en sus huecos, inserto lateral (relleno de Laplace sobre
la abertura medida) con rueda lateral y Atrás/Adelante, respaldos oscuros y suela.
Materiales e iluminación propios; no se usan texturas ni marcas de la fuente.
`render.mjs` (three.js en Chromium sin ventana) produce 72 cuadros WebP con alfa cada
5°, 4 cuadros con la pieza de cada pestaña resaltada y las posiciones de los puntos
1–4 (1,3 MB). `MouseFigure.vue` recorre los cuadros por el camino corto (~0,3 s),
funde el resaltado al llegar y oculta los puntos cuya pieza mira hacia atrás.
Licencia de la fuente (API de MakerWorld): **CC BY-NC-SA**, autora Ziemniaczenka. El
usuario eligió mantenerla: las imágenes derivadas se distribuyen bajo CC BY-NC-SA 4.0,
aparte de la GPL (`src/mouse3d/ASSETS-LICENSE.md`), con crédito visible bajo la
figura; ambos PKGBUILD declaran `CC-BY-NC-SA-4.0` e instalan el aviso. Consecuencia:
el paquete con esas imágenes no puede venderse. Los STEP no se incluyen.

**Interfaz bilingüe (23 de octubre de 2026).** La GUI ofrece español e inglés,
elegidos en Preferencias; sin elección guardada sigue el idioma del sistema. Los
textos están en `src/locales/es.ts` (fuente) y `en.ts`; `tests/i18n.test.ts` exige
las mismas claves y marcadores. El backend Rust sigue emitiendo español: etiquetas
de botones, acciones y teclas se traducen en la interfaz por su valor estable, y los
mensajes conocidos por texto exacto o patrón (un mensaje nuevo sin traducir se
muestra en español). La bandeja usa `linux-scrollmaster/locale.json` (`set_locale`) o, sin él,
`LC_ALL`/`LC_MESSAGES`/`LANG`. README principal en inglés y `README.es.md` en español.
Pendiente: aceptación física del cambio de idioma de la bandeja.

**Bandeja por StatusNotifierItem e icono propio (23 de octubre de 2026).** La
bandeja usa el backend `ksni` de `tray-icon` (D-Bus) en lugar de AppIndicator,
que en Linux no entrega clics. Un clic izquierdo (activación SNI) abre la ventana: el panel
de Omarchy (quickshell) se traga el segundo clic de un doble clic rápido, medido con
`dbus-monitor`. Icono nuevo (`src-tauri/icons/icon.svg`): un mouse con rueda y flechas
de desplazamiento, para no parecerse al de Tailwind; la barra lateral usa el mismo
dibujo. Pendiente: aceptación física del clic en la bandeja.
