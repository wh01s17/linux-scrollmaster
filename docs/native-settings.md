# N3: primeros setters nativos, sin integración en la GUI

Incremento del 20 de octubre de 2026, posterior a la aceptación real de N2.
Implementados los seis ajustes escalares, con pruebas automáticas y herramienta
Rust optativa. **Roundtrip DPI verificado físicamente: 1800 → 1850 → 1800**,
con restauración de los seis valores y perfiles intactos.
El [remapeo secundario nativo](native-button-remapping.md) también está
implementado y su roundtrip aceptado. N3 completo sigue pendiente de ampliar
la aceptación de escrituras. N4–N6 tampoco están completos; la GUI continúa usando Solaar/Python.

## Contrato implementado

`native::apply_changes` recibe transporte exclusivo, slot explícito, Unit ID
esperado, objeto de cambios y plazo monotónico. Solo admite:

| Ajuste | Validación | Setter |
|---|---|---|
| DPI | Entero estricto y opción anunciada por el sensor único | 2201, función 3, sensor 0 y valor big endian |
| SmartShift | Entero 1–50, modo dentado efectivo | 2110 función 1 / 2111 función 2; extremo 50 usa marcador ff |
| Modo de rueda | Entero 1 o 2 | Misma feature SmartShift, sin cambiar umbral expresamente |
| Inversión vertical | Booleano y capacidad anunciada | 2121 función 2, bit 2 del modo |
| Resolución vertical | Booleano y lectura disponible | 2121 función 2, bit 1 del modo |
| Inversión horizontal | Booleano y lectura disponible | 2150 función 2, bit 0 del segundo byte |

Se valida la estructura antes de cualquier E/S. Luego se realiza un diagnóstico
completo sin caché y se comprueban identidad, disponibilidad, opciones y encoders
del lote entero antes del primer setter. No se deduce identidad del slot. No se
escribe si una capacidad solicitada está ausente o no se pudo leer.

Modo precede al umbral SmartShift; no se permite ajustar un umbral oculto en giro
libre. Los cambios que ya coinciden se omiten. Los setters de ruedas usan los
bytes crudos actuales y preservan bits no solicitados, incluido reporting;
no activan diversion ni añaden un consumidor de eventos.

Después de cada ACK se vuelve a leer el diagnóstico completo usando el mismo
descriptor/lector. Se exige que el ajuste solicitado tenga el valor esperado y
que los anteriores confirmados sigan coincidiendo. Un ACK por sí solo no es éxito.
La salida `confirmed` incluye solo cambios escritos y confirmados, no no-ops.

Las escrituras no son transaccionales. Ante fallo se devuelve mensaje,
`confirmed` y `attempted`; este último puede haber llegado al firmware. No se
reenvía el setter ni se afirma rollback automático. Un error de transporte cierra
la sesión, como en N1. La prueba optativa de restauración descrita más abajo es
un flujo específico de test, distinto de la API de aplicación de cambios.

## Herramienta optativa

El ejemplo `native_settings` está separado de `native_diagnostic`; este último
conserva su catálogo de solo lectura. No hay ruta nueva en Tauri ni migración
automática de backend. Ambos ejemplos comprueban conflictos, toman el bloqueo
compartido y abren únicamente una interfaz del catálogo validado. Nunca ejecutan
la GUI como root ni cambian permisos.

```sh
# Leer primero el catálogo y el slot real: no asumir que es 1.
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_diagnostic -- --paired hidrawN

# Escritura explícita; sustituir interfaz, slot y Unit ID por los leídos realmente.
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_settings -- --apply hidrawN --slot 2 --device UNITID --changes '{"dpi":1850}'

# Prueba física OPTATIVA: cambia temporalmente DPI al valor vecino y lo restaura.
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_settings -- --roundtrip hidrawN --slot 2 --device UNITID
```

No se ejecuta ninguno de esos cambios durante las pruebas regulares. La selección
no admite una ruta arbitraria. El ejemplo se supervisa en otro proceso, con salida
máxima 128 KiB, presupuesto de 40 s para un lote y padre de 60 s. No existe un
comando genérico HID++, shell o dispatcher en el contrato de ajustes.

Estos ejemplos de migración no leen ni desarman perfiles persistidos. Requieren
la GUI/worker cerrados. La integración de N5 deberá preservar el desarme atómico
antes de editar hardware; esta herramienta no sustituye ese contrato de producto.
Un perfil armado puede reaplicar sus valores cuando se abra de nuevo la GUI.

## Roundtrip DPI y recuperación

La prueba lee primero los seis valores originales y la identidad. Selecciona
únicamente una opción DPI vecina anunciada por el dispositivo; no impone el valor
histórico de un perfil. Crea un registro temporal 0600 con baseline y restauración,
sin tocar archivos de perfiles. El intento y la restauración revalidan identidad,
capacidades y ausencia de controladores; cada setter requiere lectura posterior.

Una sección equivalente a `finally` intenta siempre restaurar el DPI original,
incluso ante error o panic del intento. Antes de restaurar abre una sesión nueva
y relee estado/identidad; no repite ciegamente el setter que falló. Después exige
que los seis valores coincidan con el baseline. Solo entonces elimina su registro
temporal. Si falla, conserva el registro e informa su ruta para recuperación.

No es una garantía frente a SIGKILL, desconexión permanente, driver bloqueado o
terminación no interrumpible del kernel. No se confunde el journal con un guardián
de restauración. N4 todavía debe implementar recuperación de botones y teclas
antes de diversion. La prueba DPI no certifica tacto, reconexión ni otros setters.

## Evidencia y pruebas

N2 real aceptado: [diagnóstico nativo](native-diagnostic.md), con Unit ID <UNIT_ID>,
modelo BLE B034, slot 2, protocolo 4.5 y seis lecturas contrastadas. Baseline de
esa aceptación: DPI 1800, SmartShift 10, modo 2, inversión/resolución vertical false
e inversión horizontal true. No se usa ese baseline para cambiar preferencias.

Fuentes de layouts: inspección estática selectiva de los setters del adaptador
Solaar instalado (`settings_templates.py`, versión y hashes en N2) y las lecturas
reales de capacidades. Implementación Rust propia; no se importa Solaar/Python
como dependencia de estas herramientas ni como oráculo de sus pruebas.

Diez casos nuevos de N3 cubren validación estricta y previa del lote, identidad,
getters sin caché, orden de modo/umbral, preservación de bits, ACK sin confirmación,
errores parciales sin reintento, extremo SmartShift y sobres IPC diferenciados.
El emulador es sintético; no constituye aceptación del firmware.

Suite al cerrar este incremento escalar: **295 pruebas aprobadas**, incluidas
84 Rust. El incremento posterior de [botones](native-button-remapping.md) eleva
el total de ese momento a **312**, con 101 Rust: 23 del host, 22 N1, 29 N2 y 27 N3.
La suite actual tras [continuar N4](native-events.md) tiene **357** pruebas. Pasan `pnpm test/build`,
`cargo check --offline --all-targets`, suite Rust completa y formato. El ejemplo
`native_settings` compila, pero no lo ejecuta la suite regular sobre hardware.

## Aceptación física DPI del 20 de octubre de 2026

El usuario autorizó expresamente el cambio temporal/restauración. El guard de
procesos pasó y se mantuvo el lease durante baseline, intento y restauración.
La herramienta Rust confirmó `confirmed:["dpi"]`, `temporary_dpi:1850`,
`original_dpi:1800` y `restored:true`. Usó el Unit ID <UNIT_ID> y el slot 2;
no se ejecutó Solaar ni Python como backend de la prueba.

| Comprobación | Resultado |
|---|---|
| DPI temporal | 1850, escrito y confirmado por lectura del dispositivo |
| Restauración DPI | 1800, confirmada por lectura nueva |
| Baseline completo | SmartShift 10, modo 2, inversión/resolución vertical false, inversión horizontal true; seis valores iguales al terminar |
| Lectura independiente posterior | Pasa en 1253 ms con identidad y los seis valores originales |
| Perfiles | Mismo SHA-256 antes y después: `2d83fccaec0f7632efec9f8e9a5920b0e33a1df72532926d21447fced47a93bd` |
| Registro temporal | Eliminado después de confirmar restauración; no quedó journal pendiente |

Esta aceptación corresponde al setter DPI y su restauración normal. Los otros
cinco setters tienen pruebas automáticas, pero aún no aceptación física con el
backend nativo. El remapeo se acepta en el [incremento posterior](native-button-remapping.md).
Siguen pendientes reconexión/desconexión durante escritura
y recuperación frente a crash; no se habilita diversion ni cambia el backend GUI.
