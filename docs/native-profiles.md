# N5: núcleo nativo de perfiles y persistencia local

Incremento del 21 de octubre de 2026, America/Santiago. `src-tauri/src/profiles/`
implementa validación, gestión local, selección de perfil efectivo y persistencia
en Rust. Es una biblioteca separada del backend HID++ y del supervisor Tauri.
**N5 sigue parcial: no reemplaza el servicio Python de la GUI ni reconcilia
hardware.** Las pruebas usan JSON y archivos temporales; no se abrieron perfiles
reales, mouse o compositor. No se cambió configuración del usuario.

## Contrato y compatibilidad

| Módulo | Responsabilidad |
|---|---|
| `profiles/model.rs` | `Document` validado, límite de bytes y versiones, referencias, selección pura de perfil |
| `profiles/operations.rs` | Planes de rename/delete/export/import_preview/import/rule_set/rule_delete/auto |
| `profiles/store.rs` | Lectura sin cambios, revisión opaca, guardado atómico y gestión local con publicación confirmada |
| `examples/native_profiles.rs` | Herramienta offline por stdin/stdout; nunca guarda ni aplica perfiles |

`Document` conserva los campos extra de raíz y perfiles, nombres e identidades
existentes y ausencia de campos opcionales. Leer una versión antigua no migra,
normaliza ni reescribe el archivo. Las referencias activas/reglas deben apuntar
a perfiles del mismo dispositivo; nombres duplicados por dispositivo fallan.

| Versión mínima | Contenido |
|---|---|
| 1 | Ajustes escalares; perfiles sin botones |
| 2 | Mappings nativos de CIDs 83/86/195, enteros 1–65535 |
| 3 | Alt/Ctrl mantenidos en laterales |
| 4 | Copiar/Pegar/Cortar/Deshacer en laterales |
| 5 | Gestos legacy en counts, cinco direcciones con acciones o `null` |
| 6 | Reglas por aplicación |
| 7 | Multimedia en gestos |
| 8 | Campo de unidades de gesto, counts o décimas de mm |
| 9 | Atajos personalizados canónicos |
| 10 | Las cuatro acciones de escritorio, en laterales o gestos |

Se reutiliza el parser cerrado `ActionBindings` para software, sin abrir salidas.
El formato persistido excluye multimedia en laterales, igual que el contrato
Python; no se amplía por admitirlo el motor experimental. Los CIDs primarios,
comandos y acciones desconocidas fallan. Esta validación no acredita que un
mapping exista en las capacidades reales del dispositivo: eso corresponde al
servicio antes de aplicar. Tampoco exige conectar el mouse para importar gestos
físicos; ejecutar esos gestos sí requiere DPI real y salida preparada.

Límites: 100 perfiles, 100 reglas, 128 KiB por documento; DPI 1–65535,
SmartShift 1–50, ratchet 1/2 e interruptores estrictamente booleanos. No se
aceptan booleanos como enteros ni números decimales como schema/DPI/mapping.
Identidad: 1–128 caracteres Unicode; nombre recortado: 1–60; aplicación: 1–256,
sin controles menores que U+0020. Las nuevas importaciones/renombrados recortan
espacios como `strip()`; los nombres existentes se conservan exactamente.

El parser exige JSON UTF-8 válido y mantiene el límite normal de anidación de
`serde_json`. `from_value` comprueba que el documento pueda volver a leerse tras
guardarlo. Se habilita `arbitrary_precision` en la dependencia ya existente:
metadatos numéricos desconocidos conservan enteros largos y decimales sin
redondearlos a `f64`. No se añaden dependencias de ejecución. La validación nativa
aplica límites de bytes UTF-8; el importador Python calculaba su presupuesto con
`json.dumps` escapando Unicode, por lo que su frontera exacta puede diferir.
JSON no estándar, sustitutos Unicode aislados, anidación excesiva, rutas inseguras
o archivos incompatibles producen error; no se reparan ni se sustituyen por vacío.

## Operaciones sin hardware

`Document::manage` devuelve otro documento, `transfer` y `changed`. El original
no se modifica, incluso ante error. Renombrar actualiza solo las referencias del
mismo dispositivo; eliminar retira sus reglas y desarma solo ese perfil. Renombrar
al mismo nombre es una operación sin escritura. Exportar conserva contenido y
versión del perfil, pero omite activación y reglas externas.

Importación y vista previa comparten el plan de nombres. Se asocian al dispositivo
seleccionado y añaden ` (importado N)` ante colisiones, truncando por caracteres.
Solo se importan ajustes y botones: se ignoran activaciones, reglas y metadatos
externos ajenos al contenido del perfil. Los existentes se conservan. La vista
previa no cambia el documento; importar aumenta la versión solo cuando hace falta
conservar la versión del documento entrante. Nunca arma perfiles ni aplica ajustes.

`auto` cambia únicamente la referencia persistida mediante una petición explícita
y exige que el perfil exista para esa identidad. Desarmar elimina esa referencia.
`effective_profile(device, application)` es selección pura: sin un perfil general
armado no elige ninguna regla; sin foco válido/coincidente usa el general. No
consulta Hyprland, abre dispositivos ni inicia reconciliación.

## Persistencia y revisión

`ProfileStore::from_environment` conserva `$XDG_CONFIG_HOME/linux-scrollmaster/profiles.json`,
con fallback a `$HOME/.config/linux-scrollmaster/profiles.json` si XDG está ausente/vacío.
Rechaza una ubicación relativa. `at` permite una ubicación explícita del servicio;
no es una ruta aceptada desde la GUI. Construcción y `load` no crean directorios,
locks ni archivos; si no existe el documento, devuelven el default vacío v1.

`load` devuelve una `Revision` opaca ligada a ruta, dispositivo/inodo del directorio
y bytes originales. `commit(revision, document)` compara esa revisión antes de
escribir y de nuevo antes del rename. Detecta cambios incluso de whitespace,
eliminación del archivo y sustitución de directorio. Otro escritor nativo produce
Busy o Conflict; el servicio debe releer, sin sobreescribir cambios silenciosamente.
`ProfileStore::manage` hace lectura/plan y persiste solo si `changed`; devuelve
éxito de una operación modificadora después de confirmar el guardado. Preview,
export y rename sin cambios no crean lock ni reescriben bytes.

El directorio propio nuevo usa 0700 y JSON/temporal/lock 0600. Las operaciones
de archivo se anclan al descriptor del directorio con `openat`/`renameat`;
se rechazan enlaces simbólicos en directorio final, JSON y lock, archivos no
regulares, hard links y escritura ajena. No se cambian permisos existentes para
forzar aceptación. `O_NONBLOCK` evita bloquear al encontrar un FIFO en lugar de JSON.

El guardado valida/codifica antes de tocar el disco, conserva un flock durante
la operación, crea temporal exclusivo, escribe/fsync, confirma revisión y hace
rename atómico. Confirma fsync del directorio y lectura exacta posterior; al crear
directorios sincroniza sus entradas en el padre. Un fallo anterior a publicación
conserva el JSON y retira solo su temporal. Puede dejar el directorio/lock propios.
Si falla después del rename, devuelve `PublishedUnconfirmed`: puede haber
publicado y debe releerse antes de aplicar o reintentar. No deshace una publicación
con otra escritura ni presenta el estado como éxito confirmado.

El lock es cooperativo entre escritores nativos. **El servicio Python actual no
comparte ese lock:** las comprobaciones de revisión no son un CAS atómico frente
a otro escritor que lo ignore. Antes de integrar en la GUI debe terminarse el
escritor Python y conservar un único propietario del archivo. Esta biblioteca no
se conecta en paralelo al worker actual. No hay journal ni una promesa de
recuperación automática tras corrupción o pérdida de energía.

## Herramienta offline

Ejemplo de validación de un documento sintético; no abre archivos de configuración:

```sh
printf '%s' '{"version":1,"profiles":[],"active":{}}' | cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_profiles -- --validate
```

`--manage` recibe `{ "document": DOCUMENTO, "request": PETICION }` por stdin.
Devuelve el plan como JSON, con `read_only:true`; `changed` describe el documento
calculado, no una escritura. No acepta rutas, setters ni restore de hardware.
La entrada está acotada a dos documentos de 128 KiB más 4096 bytes de envoltura;
cada documento conserva su límite individual. La API de persistencia se prueba
por separado con directorios desechables.

## Evidencia y continuidad

41 pruebas nuevas: 35 de biblioteca, cinco de CLI y una Python. Los fixtures
compartidos tienen 21 documentos y cuatro operaciones escritos a mano: los
validadores Rust/Python los comprueban independientemente, sin usar Solaar como
oráculo. Se cubren gates 1–10, tipos, referencias, Unicode, metadatos y precisión,
importaciones sin activación, reglas, aislamiento de identidades y no reescritura.
La persistencia cubre revisión antigua, concurrencia, permisos/enlaces, sustitución
de directorios, errores antes/después del rename y publicación sin confirmar.

**556 aprobadas**: 178 Python, 41 TypeScript y 337 Rust (268 biblioteca, 23 host,
35 integración y once CLI). Pasan `pnpm test`, `pnpm build`,
`cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`,
`cargo test --offline --manifest-path src-tauri/Cargo.toml --all-targets`, formato
y diff. Las pruebas de sockets/procesos existentes requieren salir del sandbox;
las pruebas de perfiles usan archivos temporales y no requieren permisos hidraw.

El [servicio síncrono](native-service.md) ya añade guardado desde snapshot completo,
preflight conjunto, desarmado persistido y errores parciales para ajustes/remapeos
nativos. Siguiente: integrar salidas del guardián, dueño/worker con descubrimiento
y reconexión, y contrato Tauri con un único propietario. N4 requiere aceptación
física optativa y N6 retirará Solaar/Python tras paridad. La GUI sigue con el backend anterior.
