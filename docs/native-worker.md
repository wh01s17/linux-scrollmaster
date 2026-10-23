# N5: worker Rust JSON-lines experimental

Actualización: `--serve-guarded-capture` / `WorkerConfig::with_capture` seleccionan
[captura residente](native-profile-capture.md) con salida y recuperación independiente.
Admite transiciones de plan y la GUI lo lanza con `SCROLLMASTER_BACKEND=native`
([integración](native-gui.md)); el ejecutable de la GUI atiende los mismos argumentos.

Incremento del 21 de octubre de 2026, America/Santiago. El binario
`scrollmaster-native-worker` aloja `ProfileService<NativeProfileOwner>` en un
proceso separado. **La GUI aún inicia Python/Solaar; N5 sigue parcial.** Este
incremento no abrió el mouse, compositor ni perfiles del usuario. La evidencia
de procesos usa archivos temporales y operaciones locales exclusivamente.

## Separación y lanzamiento

| Código | Responsabilidad |
|---|---|
| `src/worker.rs` | Selección explícita, framing, validación y bucle serial del servicio |
| `src/bin/scrollmaster-native-worker.rs` | CLI y stdin/stdout; errores terminales en stderr |
| `src/bridge.rs` | Supervisor de procesos compartido entre host y biblioteca |
| `src/worker_tests.rs` | Política/framing con servicio y backend sintético |
| `tests/native_worker.rs` | Binario Rust y supervisor reales, perfiles temporales |

El lanzamiento requiere `--serve --slot 1..6 --device UNITID`; acepta invertir
el orden de slot/device, sin flags duplicados, rutas, comandos o fallback.
UNITID debe ser mayúsculo de ocho hexadecimales. El almacén conserva
`$XDG_CONFIG_HOME/linux-scrollmaster/profiles.json`, o `$HOME/.config/linux-scrollmaster/profiles.json`.
No se recibe una ruta por IPC ni se crea el almacén al construir el worker.

El modo optativo `--serve-guarded` conserva ese contrato y aloja el propietario
HID en otro proceso independiente, iniciado solo cuando se necesita hardware.
No habilita software; gestión local sigue offline. `WorkerConfig::with_guardian`
selecciona el mismo modo desde un lanzador Rust de confianza. Véase
[propiedad, cierre y límites](native-profile-guardian.md).

Compilación explícita, sin iniciar procesos controladores:

```sh
cargo build --offline --manifest-path src-tauri/Cargo.toml --bin scrollmaster-native-worker
```

La CLI puede guardar perfiles y aplicar hardware: no es un diagnóstico de solo
lectura. `snapshot` también puede reconciliar un perfil previamente armado.
No ejecutar el nuevo worker junto con el escritor Python u otro controlador;
el lock de perfiles nativo sigue siendo cooperativo y Python no lo comparte.
El dueño conserva los guards/lease de hardware existentes. No se instalan
permisos, servicios, reglas udev ni preferencias de escritorio.

`Worker::spawn_native(executable, config)` prepara los argumentos cerrados desde
Rust. `Worker::start(Command)` permite al lanzador de confianza configurar el
entorno del hijo, como hacen las pruebas; no se expone a la UI. `main.rs` usa
ahora la biblioteca para el mismo supervisor y sigue llamando a `Worker::spawn`
con el script Python. `default-run = "linux-scrollmaster"` conserva el arranque
Cargo habitual al incorporar el segundo binario. El paquete final y selección
del backend en Tauri siguen pendientes.

## Protocolo

No hay mensaje inicial ni respuestas espontáneas. Cada objeto JSON terminado
en LF produce exactamente una respuesta, enviada y vaciada antes de leer otra
petición. El servicio permanece en la misma instancia. El éxito usa
`{"ok":true,"data":...}`; errores de servicio conservan fase, confirmaciones,
objetivo intentado e incertidumbre de persistencia. No se añade una revisión
global: el host Tauri continúa siendo responsable de ese contador.

| Operación | Campos además de `op` |
|---|---|
| `snapshot` | `device` opcional; no acepta foco suministrado por IPC |
| `applications` | `device` opcional; consulta catálogo/perfiles sin HID ni reconciliación |
| `set`, `remap` | `device`, `changes` |
| `save`, `restore`, `delete`, `export` | `device`, `name` |
| `auto` | `device`, `name` opcional/null para desarmar |
| `rename` | `device`, `name`, `new_name` |
| `import`, `import_preview` | `device`, `document` |
| `rule_set` | `device`, `application`, `name` |
| `rule_delete` | `device`, `application` |
| `shutdown` | Ninguno |

Se rechazan operaciones desconocidas, campos extra/ausentes y claves repetidas
en el objeto exterior. Los valores y documentos anidados pasan por el modelo de
perfiles/servicio existente; esta capa no añade un validador de duplicados anidados.
JSON/UTF-8 inválido con LF y solicitudes no válidas producen error de validación
y permiten otra petición. Nunca invocan el servicio antes de validar la frontera.

El payload admite hasta **256 KiB**, sin contar LF. Una trama truncada al EOF,
lectura fallida o exceso de tamaño es terminal: destruye el servicio antes de
intentar emitir un error, sin drenar ni ejecutar datos posteriores. EOF limpio
destruye el servicio sin respuesta adicional. `shutdown` también lo destruye
**antes** de confirmar `stopped:true`, e ignora peticiones posteriores.
Así se cierra descriptor/lease por el ciclo de vida del dueño, sin activar reporting.
Con dueño remoto se exige además confirmación de cierre y recogida del hijo;
si se agota el plazo, retorna error de etapa shutdown y no anuncia stopped.
EOF/salida fallida cierran su canal y permiten limpieza independiente, sin una
confirmación síncrona de cierre por esa vía.

La respuesta admite **2 MiB incluyendo LF**. Si no cabe, se devuelve un error
pequeño con `outcome_uncertain:true`: no se supone que la operación fue inocua
ni se repite. Se serializa antes de comprobar este límite; no es un límite duro
de toda la memoria del proceso. Un fallo de escritura/flush termina el worker y
destruye el servicio; puede haber efectos ya confirmados aunque no llegue respuesta.

## Supervisor y plazos

El servicio comparte un presupuesto de **30 s** por petición de hardware. El
[foco nativo](native-focus.md) dispone de hasta **1 s adicional en total** para
sus consultas; no amplía el presupuesto HID. La GUI
conserva sus **45 s** exteriores. El supervisor ahora establece stdin no bloqueante
y usa poll con el mismo plazo absoluto para envío y espera de respuesta: un hijo
que no lee no puede bloquear `write_all` antes del timeout. Acepta escrituras
parciales y EINTR/WouldBlock sin reenviar bytes ya escritos. El canal de respuestas
está limitado a una pendiente, además de la lectura en curso.

Timeout, error de canal, EOF, framing o contrato inválido provocan kill/wait y
estado terminal; el mismo objeto no vuelve a enviar. Un error de negocio
`ok:false` es una respuesta válida y conserva el proceso. El supervisor no repite
la petición fallida: el host puede crear otro proceso para una consulta posterior.
Una solicitud demasiado grande se rechaza localmente antes de enviar y no mata
al hijo. Kill/wait todavía depende del retorno del kernel ante I/O no interrumpible;
no se promete cierre de tiempo real ni gestión de árboles arbitrarios de procesos.

## Evidencia y continuidad

24 pruebas nuevas: doce del worker/servicio, seis del supervisor y seis del binario
como proceso. Cubren tramas/UTF-8/duplicados, recuperación tras validación, orden,
persistencia, errores parciales, EOF/shutdown, salida fallida/grande, flush antes
de la siguiente lectura, entrada mayor que la pipe y un hijo que no lee, fallos
terminales recogidos y gestión local real sin crear runtime/lease. El proceso
también prueba rechazo de software antes de adquisición y conserva el perfil armado.
Las tres pruebas anteriores del supervisor se movieron del host a la biblioteca;
no se contabilizan como nuevas. No se abrió hardware real.

**630 aprobadas**: 178 Python, 41 TypeScript y 411 Rust (339 biblioteca, 20 host,
41 integración y once CLI). Pasan pnpm test/build, cargo check/test offline con
`--all-targets`, formato y diff. Sockets/procesos de la suite existente se ejecutan
fuera de la restricción del sandbox. Auditoría: 119 archivos de código propios,
ninguno con más de 1000 líneas; máximo 977, sin nuevas excepciones.

El [foco/catálogo Rust](native-focus.md) ya consulta `j/activewindow` en cada
snapshot y añade `j/clients` para applications. Entrega al servicio la clase
observada, sin aceptarla por IPC ni conservar resultados anteriores. Las reglas
ya seleccionan perfiles con dispositivo/foco sintéticos; su aceptación física
nativa sigue pendiente. `serve` permite embedding sin escritorio; `run` utiliza
`serve_with_desktop` con el cliente Hyprland. Las demás operaciones locales no
consultan el compositor ni reutilizan una descripción cacheada.

La [compilación de botones de perfiles](native-profile-actions.md) ya permite
captura finita independiente; no habilita software en este worker.
Pendiente: integración duradera del guardián para perfiles de software y paridad completa
de respuestas/acciones antes de sustituir el worker de la GUI. No hay consumidor
de acciones, reconciliación autónoma, captura, instalación ni daemon nuevo. La reconexión mantiene su
política experimental de una interfaz Bolt y slot/Unit ID explícitos. Aceptación
física optativa y retirada de Solaar/Python en N6 siguen pendientes.


En `--serve-guarded`, el [consumidor de perfiles](native-profile-events.md) ya
atiende notificaciones durante getters/confirmaciones y sondea la misma sesión
entre peticiones. No adquiere hardware offline ni consulta foco autónomamente.
Un error idle cierra sesión/lease e invalida el ticket; la siguiente petición
recibe ese error sin ejecutarse. El vencimiento del ticket no se renueva por poll.
El dueño guardado también comprueba EOF durante inicialización y E/S. Los campos
`device.native_events` son evidencia pasiva acotada; no acreditan acciones/captura.
