# N5/N6: backend nativo en la GUI y paquete sin Solaar

Incremento del 21 de octubre de 2026, America/Santiago. La GUI ya puede usar el
backend HID++ propio de punta a punta: localización del mouse, worker nativo con
captura residente, presentación compatible con Vue y gestión de perfiles offline.
**Todo está probado con procesos reales y hardware simulado; nada de esta ruta
está aceptado físicamente.** Python/Solaar sigue siendo el backend por defecto
hasta completar el [protocolo de aceptación](native-acceptance.md).

## Selección del backend

| Valor de `SCROLLMASTER_BACKEND` | Backend |
|---|---|
| `native` | Worker HID++ propio, captura residente, sin Python |
| `solaar` | Worker Python/Solaar heredado |
| ausente u otro | Valor de compilación: heredado, o nativo con la feature `native-default` |

Se lee una vez al iniciar. Nunca hay fallback entre backends ni ejecución
simultánea. El paquete `packaging/arch-native` compila con `native-default`; el
paquete heredado no cambia. `SCROLLMASTER_BACKEND=solaar` revierte una
instalación nativa sin reinstalar (si Solaar está presente).

## Componentes

| Código | Responsabilidad |
|---|---|
| `src/main.rs` | Reparte argumentos del worker/guardián antes de Tauri; elige backend; cierre confirmado al salir |
| `src/worker.rs` (`main_with_args`) | Entrada compartida por el binario dedicado y el ejecutable de la GUI |
| `src/native_host.rs` | Localizar, lanzar `--serve-guarded-capture`, mapear respuestas, gestión offline, apagado |
| `src/presentation.rs` | Respuesta nativa → contrato `Snapshot`/`Mouse` de Vue, sin E/S |
| `native/locate.rs` | Un único receptor Bolt, tabla de emparejamiento B034 y diagnóstico N2 de solo lectura |
| `native/access.rs` | Excluye la propia cadena de procesos (GUI → worker → guardián) del bloqueo de controladores |
| `native/desktop.rs` | `desktop_actions_status`: mismo preflight sin efectos usado antes de despachar |

El ejecutable de la GUI se reejecuta a sí mismo como worker y como hijo guardián;
no hace falta instalar un segundo binario. Argumentos inválidos terminan con error
antes de abrir ventana o instancia.

## Flujo de una petición

1. Sin worker: `locate` (≤4 s) comprueba Solaar/logid/otras instancias, toma el
   lease, exige un solo receptor 046d:c548 y un único B034 que responda. Lee solo
   emparejamiento y el diagnóstico N2. Tras un fallo no se reintenta durante 10 s.
2. Se lanza el worker con `--serve-guarded-capture --slot N --device UNITID`. Slot
   e identidad provienen del localizador, nunca de la UI.
3. Cada petición pasa por el worker con el mismo límite de 45 s. Un fallo de
   transporte descarta el worker (el guardián independiente restaura) y no se
   repite la petición; la siguiente vuelve a localizar.
4. Respuestas correctas se transforman con `presentation`: `protocol` y `battery`
   numéricos, ajustes `{value,available,choices,error}`, botones 83/86/195 con
   modificadores, atajos, catálogo de combinaciones, acciones Hyprland (deshabilitadas
   si su preflight falla) y gestos solo con capacidades divert + raw XY.
5. Operaciones locales no leen HID: muestran la última lectura real del snapshot
   anterior en lugar de vaciar el dispositivo hasta el siguiente sondeo.

Sin mouse localizado, `snapshot` devuelve los perfiles guardados con el motivo en
`diagnostic`; las operaciones locales usan el mismo servicio y validación estricta
del worker con un backend que rechaza cualquier petición de hardware. `set`,
`remap`, `save` y `restore` fallan con ese motivo.

Al cerrar la app se envía `shutdown` y se espera `stopped` (hasta 25 s), de modo
que el guardián restaura el reporting antes de salir. Un error se registra en
stderr; no se marca como restaurado.

## Paridad de semántica

- `remap` parcial conserva los botones no incluidos, también los de software, como
  hacía Python. Un perfil que no menciona un botón tampoco lo libera.
- Un control capturado por este dueño puede volver a una función nativa en el mismo
  lote: se valida como liberado y se remapea después de la liberación confirmada.
- Cambiar de plan (foco/perfil/remap) libera y reactiva bajo el mismo dueño; el mismo
  plan no repite setters. Ver [captura residente](native-profile-capture.md).
- Tras un fallo con limpieza confirmada, la siguiente petición reconecta con un
  preflight nuevo; nunca se repite la petición fallida. Limpieza no confirmada o
  resultado IPC desconocido siguen siendo terminales.
- `gesture_feedback` todavía es `null` en el backend nativo; el editor de gestos
  funciona sin la vista previa de dirección. Pendiente.

## Acceso hidraw propio (N6)

`packaging/udev/70-linux-scrollmaster.rules` concede `uaccess` (ACL del usuario de
la sesión activa) **solo** a la interfaz 02 del receptor 046d:c548, la HID++ con
página de uso de fabricante 0xFF00. Las interfaces 00/01 (teclado/mouse) no se
conceden: la regla de Solaar sí concede las tres. Sin MODE/GROUP/0666. Usa
propiedades de `60-persistent-hidraw.rules` y se ordena antes de `73-seat-late`.

Evidencia en el equipo de desarrollo (solo lectura de sysfs/udev): hidraw5/6/7 son
las interfaces 00/01/02; hidraw7 empieza con `06 00 ff` y tiene `ID_USB_INTERFACE_NUM=02`.
`udevadm verify` aprueba la sintaxis. La regla **no se instaló**: su efecto real
se comprueba en la aceptación desde instalación limpia.

## Paquete nativo

`packaging/arch-native/PKGBUILD` (`linux-scrollmaster-native`, `conflicts`/`provides`
`linux-scrollmaster`) no depende de Python ni Solaar, no instala `bridge/`, compila
con `--features native-default` e instala binario, regla udev, `.desktop`, icono y
licencia. `prepare.py --variant native` genera el kit con checksum desde un commit
limpio. No se ejecutó `makepkg` ni se instaló durante este incremento.

## Evidencia

Pruebas nuevas: localizador (2), host con almacén temporal y workers de guion (5),
presentación (5), cadena de procesos (1), entrada del ejecutable GUI como worker (1),
servicio→dueño→captura con foco y reconexión (2), remap parcial (1), política de
reconexión del dueño (1, sustituida) y del proxy (2), catálogo offline (1),
entrega lenta en el sondeo de la sesión (1) y empaquetado/udev (5 Python).
Totales del incremento en [plan.md](../plan.md).
