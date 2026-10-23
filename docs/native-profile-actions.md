# N5: botones de perfiles y guardián finito

Incremento del 21 de octubre de 2026, America/Santiago. Los mapas `buttons` del
contrato persistido ya se compilan a remapeos de firmware y acciones de software
separados. El guardián experimental puede capturar estos últimos durante un
intervalo finito y restaurar reporting al terminar. **N5 sigue parcial: el servicio
y la GUI todavía no ejecutan perfiles de software con el backend Rust.**
La evidencia nueva usa procesos reales y mouse HID++ sintético; no se abrió
hardware, compositor ni perfiles del usuario.

## Separación y contrato

| Código | Responsabilidad |
|---|---|
| `native/profile_buttons.rs` | Compilar el mapa persistido, separar destinos nativos y software, comprobar remapeos nativos existentes |
| `native/guardian_config.rs` | Construcción sin E/S, conjunto exacto de recuperación y configuración cerrada del hijo |
| `native/guardian.rs` | Validar requisitos en la sesión única, activar, confirmar ready, cerrar salida y restaurar |
| `examples/native_guardian.rs` | Opción explícita `--profile-buttons-json` y revalidación al iniciar el hijo |
| `native/profile_buttons_tests.rs`, `guardian_profile_tests.rs` | Contrato y ciclo completo con perfiles/frames/procesos sintéticos |

`ProfileButtonPlan::parse` recibe exclusivamente el objeto `buttons`, no un
documento ni un perfil completo. Reutiliza el validador de persistencia: CIDs
83/86/195, destinos nativos enteros, Alt/Ctrl y acciones de edición/atajos/escritorio
en laterales, y gestos con cinco direcciones, multimedia y unidades originales.
No acepta multimedia directamente en laterales, aunque el motor experimental
aislado tenga esa capacidad. No convierte números o unidades ni añade acciones.
Un documento completo debe validarse primero con `Document`: el plan no sustituye
la comprobación de versión ni la selección explícita de identidad/perfil.

El plan conserva una copia del mapa original y ofrece `native_mappings`,
`capture_bindings`, `actions` y `capture_controls`. Los destinos enteros nunca
entran en el motor de acciones ni en el conjunto de botones a interceptar. Un
mapa solo nativo puede compilarse, pero no iniciar captura. Un schema 1 sin
botones no necesita este plan. Construirlo no lee archivos, adquiere lease, abre
HID/Wayland, guarda, selecciona ni arma un perfil.

## Captura explícita

`GuardianConfig::capture_profile_buttons(interface, slot, id, buttons, duration)`
deriva el conjunto de recuperación exclusivamente de los botones de software.
`with_profile_buttons` exige que el conjunto ya seleccionado coincida exactamente.
Conserva los límites de selección y duración del guardián, con JSON de arranque
de hasta 8192 bytes. Cambiar a keyboard-check o asignaciones independientes
descarta los requisitos anteriores del perfil.

La CLI admite `--capture ... --profile-buttons-json OBJETO [--refresh]` como
alternativa excluyente a `--bindings-json`. No se admite con `--observe` ni con
`--keyboard-check`. Esta opción recibe un mapa explícito y **puede activar
temporalmente diversion/raw XY y emitir acciones**. No aplica ajustes escalares,
no escribe remapeos nativos y no carga automáticamente el perfil armado del usuario.
No se ejecutó una captura física durante este incremento.

Ejemplo del objeto, sin ejecutar hardware:

```json
{"83":86,"86":"modifier:ctrl"}
```

En este caso solo se registra/intercepta CID 86. CID 83 debe anunciar ya el
destino nativo 86, sin error ni flags de interceptación. Una discrepancia detiene
el arranque antes del primer setter de captura; no se omite ni se corrige el
remapeo silenciosamente. La comprobación utiliza el snapshot fresco del dueño,
en su misma sesión y bajo el lease del guardián.

La activación existente vuelve a leer identidad, capacidades, destinos y flags
de todo el catálogo antes de cada setter y al terminar. Un cambio nativo durante
el lote lo detiene, conserva las confirmaciones parciales y pasa a restauración;
no se anuncia ready. La comprobación final del perfil también exige los destinos
nativos antes de anunciarlo. Los refresh posteriores conservan la comparación
completa del catálogo. Las acciones durante activación permanecen bloqueadas.

El hijo recibe el modo cerrado `capture-profile` y el mapa original. Revalida el
plan y el conjunto exacto de recuperación; no recibe rutas o documentos por el
canal de control. Ready contiene las asignaciones software y `profile_buttons`;
el padre contrasta ambos con su configuración. Un hijo que omite los requisitos
nativos no satisface ese arranque. El resultado terminal conserva el mapa como
evidencia histórica, sin presentarlo como una lectura vigente.

Se mantienen cierre de salida antes de restauración, descriptor/lector único,
lease hasta recuperar y ausencia de reintentos inciertos. La recuperación toca
solo los CIDs registrados y preserva los remapeos actuales, incluso si otro actor
los cambió. Los límites de pérdida de dispositivo y muerte del guardián continúan
en [captura nativa](native-capture.md).

## Evidencia y continuidad

14 pruebas nuevas: cinco de compilación/validación, dos de configuración, cuatro
con procesos del guardián y protocolo sintético, y tres de CLI. Verifican mapas
mixtos, formatos de schemas 2–10, distancia y atajos conservados, rechazo del mapa
solo nativo, errores antes de diversion, cambio nativo durante activación parcial,
acciones únicamente en software, restauración y rechazo de ready incompleto.
Las pruebas existentes de captura y SIGKILL siguen pasando; no constituyen
aceptación física nueva.

**668 aprobadas**: 178 Python, 41 TypeScript y 449 Rust (371 biblioteca, 20 host,
44 integración y catorce CLI). Pasan `pnpm test`, `pnpm build`, `cargo check/test
--offline --manifest-path src-tauri/Cargo.toml --all-targets`, formato y diff.
La suite completa de sockets sintéticos se ejecutó fuera de la restricción del
sandbox tras errores de permisos dentro. Auditoría de 128 archivos de código
propios: ninguno supera 1000 líneas; máximo 971. Se separaron configuración del
guardián y configuración de fixtures para cumplir ese límite sin excepciones.

Siguiente: propietario/guardián duradero con servicio de perfiles, aplicación
conjunta de ajustes/remapeos y software, cambios de perfil/foco, reconexión y
recepción de eventos entre peticiones. El dueño actual sigue rechazando perfiles
de software antes de adquirir hardware; esta herramienta finita no sustituye esa
garantía ni puede ejecutarse concurrentemente con otro propietario. Después,
completar paridad del worker/Tauri y N6 sin Solaar/Python. La aceptación física
nativa continúa optativa y pendiente.
