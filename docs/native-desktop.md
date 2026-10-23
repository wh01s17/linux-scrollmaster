# N4: acciones nativas de escritorio

Incremento del 21 de octubre de 2026, America/Santiago. `native/desktop.rs`
implementa salida Hyprland propia en Rust, conectada al motor de botones/gestos
y a la [captura optativa del guardián](native-capture.md). **Pruebas sintéticas;
aceptación física pendiente.** No se abrió el compositor ni el mouse reales.
La GUI conserva Solaar/Python; este incremento no configura el escritorio.

## Catálogo y separación de salida

| Identificador | Petición fija después de `/dispatch ` |
|---|---|
| `desktop:workspace-previous` | `hl.dsp.focus({ workspace = "r-1" })` |
| `desktop:workspace-next` | `hl.dsp.focus({ workspace = "r+1" })` |
| `desktop:fullscreen` | `hl.dsp.window.fullscreen({ mode = "fullscreen", action = "toggle" })` |
| `desktop:maximize` | `hl.dsp.window.fullscreen({ mode = "maximized", action = "toggle" })` |

`DesktopAction` es un enum cerrado; ningún parámetro del usuario se interpola en
Lua. `ActionBindings` admite esos cuatro IDs en laterales 83/86 y las cinco
direcciones de 195. `TapAction` conserva tipos separados para teclado y escritorio.
`KeyboardAction` sigue rechazando acciones de escritorio; no hay shell, proceso
`hyprctl`, comandos genéricos ni simulación de atajos para mover espacios.

Los flancos, supresión inicial, cancelación y consumo del gesto antes de emitir
son los del motor existente. Una acción de escritorio no cambia los modificadores
mantenidos ni produce teclas. El conjunto de salidas nativo todavía requiere
teclado virtual Wayland autorizado, incluso con asignaciones solo de escritorio.
Las asignaciones de captura siguen fijas desde el arranque; IPC solo permite
estado, refresh y cierre. Esta capa no interpreta perfiles ni schemas 1–10.

## Preflight y sesión

Si hay alguna acción de escritorio, construir el motor exige dos consultas:

1. `j/version`, JSON con versión `v?0.(55|56).PATCH` y sufijo opcional `-`/`+`.
   Las otras series, respuestas malformadas y versiones ausentes se rechazan.
2. `/eval return assert(type(hl.dsp.focus) == "function" and type(hl.dsp.window.fullscreen) == "function")`.
   Es una comprobación constante de tipos; no ejecuta dispatchers. Solo `ok`,
   con espacios ASCII opcionales alrededor, confirma capacidades Lua.

La comprobación ocurre antes de que el guardián pueda activar diversion. También
se repite al aceptar snapshots iniciales/finales/refresh y al reemplazar
asignaciones mediante la API del motor. Antes de **cada acción** se repiten ambas
consultas y después se envía la petición fija. No se conserva una capacidad exitosa
para otra acción. Un motor con solo teclado no consulta Hyprland.

Cada operación resuelve `XDG_RUNTIME_DIR/hypr/HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`.
Exige runtime absoluto y firma de un componente normal, sin barras ni controles.
Conexión no bloqueante y CLOEXEC; `SO_PEERCRED` debe corresponder al usuario actual.
No hay búsqueda de otras instancias ni fallback. Las tres conexiones de una acción
usan una única ruta resuelta; la siguiente acción vuelve a resolver su sesión.
Si cambia el compositor, una comprobación anterior no autoriza otro destino.

El dialecto coincide con el [contrato Python existente](desktop-actions.md).
El código oficial de [Hyprland v0.56.2, `HyprCtl.cpp`](https://github.com/hyprwm/Hyprland/blob/v0.56.2/src/debug/HyprCtl.cpp)
confirma que `eval` requiere el gestor Lua y `dispatch` usa `hl.dispatch(...)` con
ese gestor. No se amplía compatibilidad por adivinar dialectos de otras versiones.

## Límites y fallos

Cada intercambio tiene un plazo absoluto de 1 s que incluye conexión, escritura y
lectura hasta EOF; respuestas parciales no renuevan ese plazo. Máximo 1 MiB para
versión y 4096 bytes para probe/dispatcher. Una acción usa hasta tres intercambios.
Solo `ok` completo confirma aceptación; prefijos, bytes extra y confirmaciones
tardías fallan. Un `ok` no acredita el resultado visible del cambio de ventana.

Un fallo puede ocurrir después de ejecutar el dispatcher: **no se reintenta** ni
se intenta deshacerlo con otra orden. El motor consume el flanco/gesto antes de la
salida, cierra teclado una sola vez y propaga el fallo al guardián. La recuperación
de reporting sigue sobre el transporte original y lease retenido; una liberación
de teclado sin confirmar no impide intentarla. Un fallo de preflight inicial no
envía setters de captura ni anuncia `ready`.

El cliente espera hasta 75 s al arrancar captura con escritorio, frente a 65 s con
solo teclado y 45 s de observación. Ese margen incluye preflights; no amplía los
plazos de sesión, HID, activación o restauración. Captura sigue siendo una
herramienta finita de 1–30 s, no el servicio de reconciliación de N5.

## Evidencia y continuidad

29 casos nuevos: 16 de protocolo/socket Unix, diez del motor/enrutamiento y tres
de procesos del guardián. Cubren catálogo fijo, versiones, ruta de sesión,
capacidad Lua, preflight sin dispatch, revalidación por acción, fragmentación,
EOF, límites, goteo/timeout absoluto, ACK incierto sin reintento, flancos y gestos,
modificadores conservados, cancelación y cierre. Los procesos prueban captura y
restauración, rechazo antes de escribir sin escritorio y fallo de dispatcher con
modificadores mantenidos. Todos los peers y dispositivos son sintéticos; no se
requiere mouse, root, red ni permisos hidraw o de teclado virtual.

**515 pruebas aprobadas**: 177 Python, 41 TypeScript y 297 Rust (233 biblioteca,
23 host, 35 integración y seis CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml --all-targets`, formato
y diff. Las pruebas con sockets/procesos privados se ejecutan fuera del sandbox.

N4 permanece parcial hasta aceptación física voluntaria de captura, acciones,
cancelación, cierre y recuperación. N5 debe integrar servicio, perfiles,
reconciliación y Tauri conservando contratos/datos. N6 retira Solaar/Python y
valida instalación limpia. No trasladar la evidencia física del backend anterior
al backend nativo ni activar el experimental por defecto.
