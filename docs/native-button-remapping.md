# N3: remapeo secundario nativo

Incremento del 20 de octubre de 2026. `native/buttons.rs` implementa lectura y
reasignación de funciones de firmware por HID++ 1B04. La herramienta aislada
`native_buttons` no importa Solaar/Python, no carga perfiles y no conecta este
backend experimental a la GUI. **Roundtrip Atrás→Adelante→Atrás aceptado en Bolt**,
con lectura independiente final y restauración de botones y seis ajustes.

## Contrato y límites

- Fuentes editables: CIDs 83 (Atrás), 86 (Adelante) y 195 (gestos nativos).
  No se permite editar los clics principales ni asignar acciones de software.
- Identidad estable esperada y catálogo leídos en la misma sesión exclusiva.
  Índice 1B04 resuelto dinámicamente; revisiones admitidas 2–5, feature sin flags,
  tabla de 1–64 controles, sin CIDs duplicados ni grupos fuera de rango.
- Cada fuente debe anunciar control físico de mouse reprogramable. Sus destinos
  se derivan de los grupos/máscara anunciados; su propia función siempre figura.
  Las funciones de clic pueden ser destinos si el firmware las anuncia.
- Reporting desconocido o interceptado bloquea el control: cualquier flag distinto
  de cero se rechaza. Errores de lectura no se sustituyen por valores de fábrica.
- Se valida todo el lote antes del primer setter. Una nueva lectura inmediatamente
  anterior comprueba que el mapping y reporting no hayan cambiado.
- Setter función 3: `[CID_hi, CID_lo, 0, destino_hi, destino_lo, 0]`.
  Todos los flags de reporting y bits de cambio válido permanecen en cero.
  No habilita diversion, raw XY, teclas ni gestos de software.
- ACK con eco y diagnóstico posterior sin caché deben confirmar la función.
  También se comprueban las asignaciones confirmadas previamente en el lote.
  Un fallo devuelve `confirmed` y `attempted`; no reintenta ni promete atomicidad.
- Destino cero significa conservar el mapping y se rechaza como entrada. Restaurar
  la función propia envía su CID explícito. Un marcador de fábrica cero leído se
  interpreta como función propia: la garantía es semántica, no igualdad de bytes.

La integración de N5 deberá desarmar de forma durable los perfiles antes de una
escritura manual y mantener un único propietario. Esta herramienta requiere la
GUI y otros controladores cerrados, comprueba Solaar/logid y usa el lease común.

## Herramienta optativa

Sin argumentos termina con uso/error, sin abrir hardware. Lectura explícita:

```sh
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_buttons -- --read hidraw7 --slot 2 --device <UNIT_ID>
```

Prueba física que requiere autorización previa y modifica temporalmente Atrás:

```sh
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_buttons -- --roundtrip hidraw7 --slot 2 --device <UNIT_ID>
```

`--apply` usa la misma selección y añade `--changes '{"83":86}'`. Es una
reasignación efectiva hasta que otra operación la restaure; no tiene restauración
automática. No ejecutarla como una consulta ni durante el uso de perfiles.

El roundtrip exige baseline completo de tres botones y seis ajustes. Escribe un
registro privado con creación exclusiva y sincronización en
`/tmp/scrollmaster-n3-button-recovery-<pid>.json` antes del intento. La sección
finally intenta restaurar únicamente Atrás, revalidando identidad y capacidades,
incluso ante error o panic capturado. Solo elimina el registro después de comprobar
el baseline completo. Si falla la restauración, conserva la ruta para recuperación.
No garantiza restauración ante SIGKILL, caída del sistema o desconexión: el registro
no es un guardián. La supervisión acota el proceso; N4 deberá resolver la recuperación
de eventos/diversion antes de habilitarlos.

## Procedencia y pruebas

Contrato contrastado con la [especificación Logitech 1B04 v2](https://lekensteyn.nl/files/logitech/x1b04_specialkeysmsebuttons.html)
y análisis estático selectivo del archivo instalado
`/usr/lib/python3.14/site-packages/logitech_receiver/hidpp20.py`:
`ReprogrammableKeyV4`, `KeysArrayV4`, `KeyFlag`, `MappingFlag`, lectura y setter
de reporting. SHA-256 del archivo:
`b6dfbd88bba8dcbf6b3eb2de5ea1345c2b02f56a67e23799a1b2a5c7f635aaa7`.
Implementación Rust propia; no se ejecuta ni importa ese módulo en las pruebas.
Aceptar versiones 2–5 es una restricción de implementación; la aceptación física
registrada corresponde únicamente a la revisión 5 del mouse probado.

Diecisiete pruebas sintéticas nuevas cubren grupos dinámicos, validación completa,
protección de fuentes principales, identidad, versiones, tablas inválidas,
flags desconocidos, cambios durante preflight, bytes del setter, restauración
explícita, no-op, ACK incorrecto/sin efecto y fallos parciales sin reintento.
Los fixtures son construidos para probar el contrato; no son capturas físicas.

Suite al cerrar este incremento: **312 pruebas aprobadas**, incluidas 101 Rust (53 biblioteca,
23 host y 25 integración). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets` y
`cargo test --offline --manifest-path src-tauri/Cargo.toml`.

## Evidencia física Bolt

Con autorización expresa del usuario y sin controladores competidores, el receptor
`hidraw7`, slot 2, Unit ID <UNIT_ID>, modelo BLE B034/HID++ 4.5, anunció
1B04 índice 9/revisión 5 y ocho controles. Las tres fuentes editables estaban
disponibles con grupo 2, máscara 3, capacidades `0x0531` y reporting cero.
Cada fuente anunció siete destinos: 80, 81, 82, 83, 86, 195 y 196.

| Comprobación | Resultado |
|---|---|
| Cambio temporal | Fuente 83 → destino 86, `confirmed:["button:83"]` |
| Restauración | Fuente 83 → destino 83, `restored:true` |
| Lectura independiente posterior | 83→83, 86→86, 195→195; todos con flags cero |
| Seis ajustes finales | DPI 1800, SmartShift 10, modo 2, inversión/resolución vertical false, inversión horizontal true |
| Perfiles | SHA-256 idéntico antes/después: `2d83fccaec0f7632efec9f8e9a5920b0e33a1df72532926d21447fced47a93bd` |
| Recuperación | Registro eliminado tras confirmar baseline; sin registros de esta prueba pendientes |

Salidas locales de la prueba y lectura final:
`/tmp/scrollmaster-native-buttons-roundtrip.json` y
`/tmp/scrollmaster-native-buttons-final.json`; son temporales, no fixtures de la suite.
La aceptación confirma el setter y su lectura posterior; no simula una pulsación
ni verifica el efecto de la función dentro de otra aplicación. No cubre escrituras
de todas las combinaciones, desconexión durante escritura ni recuperación por crash.
Los otros cinco setters escalares aún requieren aceptación física propia. N3 sigue
parcial. N4 inicia después la [recepción pasiva](native-events.md); guardián,
acciones y N5/N6 siguen pendientes. La GUI continúa con Solaar/Python.
