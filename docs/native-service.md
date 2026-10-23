# N5: servicio síncrono y reconciliación nativa

Actualización: la admisión es consultable por backend. El modo separado
[captura residente](native-profile-capture.md) permite un plan fijo de software;
el adaptador nativo de este documento sigue rechazándolo. El desarmado previo
a apply y la verificación independiente del recibo se conservan.

Incremento del 21 de octubre de 2026, America/Santiago. `service.rs` coordina el
núcleo de perfiles con `NativeProfileBackend`, que utiliza los getters/setters
Rust existentes sobre una sesión única. **N5 sigue parcial y la GUI conserva
Solaar/Python.** Este incremento se probó con perfiles temporales y protocolo
HID++ simulado; no se abrió el mouse, compositor o archivo de perfiles reales.

## Separación y propiedad

| Código | Responsabilidad |
|---|---|
| `service.rs` | Orquestación de perfiles, desarmado persistido, selección y reconciliación por petición |
| `profiles/operations.rs` | Guardado puro desde descripción completa, conservando metadatos |
| `profiles/store.rs` | Revisión/publicación confirmada y comprobación de revisión actual |
| `native/profile_backend.rs` | Preflight conjunto y aplicación de ajustes/remapeos nativos sobre `Session<T>` |
| `service_tests.rs` | Política de servicio con archivos y estado de dispositivo sintéticos |
| `native/profile_backend_tests.rs` | Backend Rust real con respuestas HID++ escritas a mano |

`ProfileService` tiene un backend y un `ProfileStore`. Se construye explícitamente;
no inicia un proceso, bucle de polling, temporizador ni worker Tauri. El adaptador
`NativeProfileBackend` recibe propiedad de una `Session<T>` existente. Puede usarse
mediante el [dueño nativo](native-profile-owner.md), que abre al solicitar hardware,
mantiene lease/controladores y permite reconexión en una petición posterior.
El uso directo del adaptador aún exige un dueño exterior. La exclusión de otros
escritores de perfiles pertenece a la integración; no hay fallback a Solaar.

El adaptador concreto está limitado al slot Bolt 1–6 y Unit ID mayúsculo de ocho
hexadecimales. Sus lecturas actuales exigen el catálogo 1B04 del modelo objetivo,
incluso para describir un perfil sin botones. No acredita Bluetooth ni variantes
sin ese catálogo. Una sesión cerrada permanece cerrada: el dueño la destruye y
construye otra tras nuevo descubrimiento y validación, en una petición posterior.
No se reabre dentro del adaptador ni se reintenta un apply fallido.

## Flujo de operaciones

`handle(request, application)` admite snapshot/save/set/remap/restore y las
operaciones locales auto/rename/delete/export/import_preview/import/rule_set/
rule_delete. Solo gestiona un dispositivo conectado por instancia. Las operaciones
con hardware exigen su identidad; la gestión de otros perfiles puede hacerse
localmente sin llamar al backend. `applications` ya devuelve perfiles/activación
sin adquirir HID ni reconciliar; el [worker](native-focus.md) añade catálogo y
consulta el foco con el cliente Rust. Adaptación completa al contrato Tauri y
aceptación física del foco nativo siguen pendientes.

1. Lee y valida el documento persistido; un error no se convierte en un archivo
   vacío ni permite continuar al hardware.
2. Para set/remap/restore construye `Changes` cerrado y exige preflight del conjunto
   completo. Un botón incompatible impide cambiar incluso los ajustes escalares.
3. Comprueba la revisión actual y desarma el perfil general de esa identidad
   mediante publicación confirmada **antes del primer setter**. Las reglas quedan
   guardadas pero no son efectivas sin perfil general armado. Un fallo de disco o
   revisión antigua impide aplicar. Un preflight fallido no desarma.
4. El backend vuelve a consultar el conjunto antes de aplicar, usa los setters
   nativos y sus confirmaciones sin caché, y finalmente relee ajustes y botones.
   El servicio también contrasta identidad y todos los valores solicitados con
   la descripción final. Un ACK o una confirmación parcial no bastan para éxito.

Las máscaras de remapeo mantienen los bits de cambio/reporting a cero; este
adaptador no activa diversion. **Las asignaciones de software se rechazan antes
de getters/setters**, aunque el modelo de perfiles acepte schemas 1–10. Alt/Ctrl,
atajos, multimedia, gestos y escritorio deben integrarse con el ciclo de vida del
guardián de N4; no se aplican parcialmente cambiando primero los DPI.

`save` exige seis lecturas disponibles y válidas de ajustes. Si hay catálogo de
botones, todos los controles entregados deben estar disponibles y ser únicos.
No elimina un CID previamente guardado si falta en una lectura posterior. Exige
coincidencia de identidad y conserva campos extra del perfil existente, metadatos
del documento, reglas y activación explícita anterior. Un perfil nuevo no se arma.
La versión aumenta solo según el contenido guardado. Un conflicto de persistencia
conserva el archivo nuevo del otro escritor y devuelve error.

## Reconciliación y errores

Una petición snapshot lee el estado actual. Solo reconcilia si hay perfil general
armado para esa identidad; el foco suministrado puede elegir una regla guardada.
Sin foco coincidente usa el general. Esta capa recibe el foco como dato y no
consulta el compositor. Compara de nuevo en cada petición: no conserva una marca
de éxito que oculte cambios externos o recuperación tras una interrupción.

Los valores ya presentes se verifican sin reenviar setters. Otro poll puede leer
el estado otra vez y decidir qué cambios idempotentes faltan; no hay reenvío ciego
de una petición incierta ni reproducción de eventos. Una aplicación automática
fallida no anuncia reconciliación y retira valores de dispositivo potencialmente
obsoletos. Los perfiles y la elección activa se conservan. Si falla solo preflight,
puede devolver la lectura inicial sin afirmar que aplicó el perfil.

Los errores incluyen fase, mensaje, confirmaciones históricas, objetivo intentado
y persistencia sin confirmar. Las confirmaciones históricas no prueban estado
actual ni rollback. En ediciones/restauraciones fallidas tras el desarmado, ese
desarmado permanece. Un fallo de publicación posterior al rename conserva la
distinción `PublishedUnconfirmed` y no permite continuar al hardware.

El presupuesto de cada petición de hardware es único, mayor que cero y hasta
30 s. Getters, preflight, setters y lectura final comparten el mismo `Instant`;
se rechazan callbacks tardíos. La persistencia no convierte syscalls de disco en
operaciones cancelables; se verifica el plazo antes de continuar al hardware.
La exclusión de escritores sigue siendo cooperativa: los guards de revisión no
son un CAS atómico contra el worker Python que ignore el lock. No ejecutar ambos
escritores o controladores en paralelo durante la futura integración.

## Evidencia y continuidad

29 pruebas nuevas: 19 de servicio y diez de backend/protocolo. Cubren desarmado
antes de escribir, errores y carreras de persistencia, perfiles por identidad/foco,
poll repetido, lecturas incompletas, timeout, recibos incorrectos, fallos parciales,
preflight de ambos tipos de ajustes, ACK sin efecto, deriva de DPI durante remapeo
y rechazo de software antes de configurar parcialmente. Un caso conecta archivo
persistido → servicio → backend Rust → frames HID++ sintéticos y confirma que
el segundo poll no repite setters. No constituye aceptación física ni reconexión
automática del transporte.

**585 aprobadas**: 178 Python, 41 TypeScript y 366 Rust (297 biblioteca, 23 host,
35 integración y once CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml --all-targets`, formato
y diff. Los sockets/procesos de la suite existente se prueban fuera del sandbox.

La regla del proyecto en `AGENTS.md` limita los archivos de código a 1000 líneas,
con excepciones justificadas por escrito. Se revisaron 111 archivos propios de
Rust/Vue/TS/JS/Python/C/CSS/HTML/shell, excluyendo dependencias y generados: ninguno
supera el límite; el mayor tiene 977. Los módulos y pruebas nuevos están separados
por responsabilidad y no requieren excepción.

El [incremento del dueño](native-profile-owner.md) ya añade arranque offline,
descubrimiento/reconexión y lease/controladores, con 21 pruebas adicionales.
El [worker JSON-lines](native-worker.md) ya ejecuta este servicio en proceso
separado con límites de tramas, selección explícita y supervisor compartido.
El [foco/catálogo Rust](native-focus.md) ya conecta consultas Hyprland con reglas,
probado con peers y dispositivo sintéticos, sin aceptación física nueva.
Siguiente: salidas del guardián para perfiles con acciones.
Después, adaptación y sustitución del worker Tauri, conservando contratos y un solo
propietario del dispositivo y archivo. La captura nativa requiere aceptación
física optativa; N6 retirará Solaar/Python tras paridad e instalación limpia.


El [propietario independiente de perfiles](native-profile-guardian.md) ya conecta
este dueño al servicio mediante `--serve-guarded`, con descriptor/lease en el hijo,
preparación de un solo uso y cierre confirmado. El padre conserva la persistencia.
La ruta es optativa y solo aplica ajustes/remapeos nativos; acciones, consumidor
y restauración duraderas siguen pendientes. La GUI conserva el backend actual.
