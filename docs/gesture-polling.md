# Gestos descartados durante el sondeo periódico

26 de octubre de 2026, America/Santiago.

El usuario reporta que necesita tres o cuatro intentos para que funcionen los
gestos, que después responden normalmente y que el problema reaparece tras un
rato, como si hubiera suspensión. Se revisaron README, plan y arquitectura,
captura residente, consumidor, transporte y motor de acciones nativos.

## Defecto reproducido

El host consulta periódicamente el estado del dispositivo (cada cinco segundos
después de terminar la consulta anterior). `NativeProfileBackend::current` llama
a `ProfileEvents::before_query`, que llega al motor `Runtime::before_query`.
Antes de este cambio, ese hook ejecutaba `set_dpi(None)` aunque los DPI reales
no hubieran cambiado. Para los gestos en décimas de milímetro, esto eliminaba el
recorrido en curso y suprimía el botón 195 hasta soltarlo. Además, todas las
pulsaciones recibidas durante la consulta quedaban suprimidas. Repetir el gesto
durante esa ventana no podía ejecutarlo.

Dos regresiones con sesión/decoder/motor reales y transporte simulado fallaron
antes de la corrección: gesto mantenido a través de cuatro lecturas idénticas,
y dos gestos completos intercalados durante un getter. La primera no emitía nada;
la segunda solo emitía el atajo lateral, perdiendo ambos gestos.

Esto acredita un defecto de software compatible con la falta intermitente de
respuesta. No demuestra que explique por completo el incidente físico, ni que el
mouse o USB estén entrando en suspensión. No se midió el tiempo real de las
consultas ni se abrió hardware para este incremento.

## Corrección

El motor conserva la escala previamente confirmada para calcular recorrido
provisional durante las consultas. Retiene las acciones físicas que se reconocen
en ese intervalo y solo las entrega cuando el snapshot completo valida los mismos
DPI y las capacidades. Así, una lectura idéntica conserva un gesto en curso y
permite reconocer nuevas pulsaciones recibidas durante la lectura.

Los botones laterales siguen procesándose inmediatamente. Los gestos fuera de
una consulta siguen ejecutando su acción al reconocer la dirección. Durante una
consulta puede haber demora hasta su confirmación; no se promete latencia cero.
No se cambia el transporte, el ritmo de sondeo ni la administración de energía.

Cambiar o perder DPI, fallar la validación, cancelar, cambiar la asignación o
cerrar descarta las acciones provisionales. Un cambio de DPI cancela además el
gesto mantenido y exige soltarlo. La cola está limitada a 64 acciones; agotarla
cierra la salida por el camino de error y recuperación existente. Al entregar,
se consume la cola antes de emitir: un fallo incierto no repite ninguna acción
ni emite las restantes. Se conserva la omisión del primer raw XY de 1B04 v5.

## Verificación

Nueve regresiones nuevas cubren conservación del recorrido, gestos intercalados,
confirmación sin duplicados, cambio/pérdida de DPI, snapshot inválido, cierre,
cancelación, reasignación, fallo de salida y límite de memoria. La prueba del
backend residente cruza cuatro snapshots con el botón mantenido, comprueba una
única acción y confirma que no se repiten setters de captura. El cierre sigue
restaurando los flags temporales en el dispositivo simulado.

- `pnpm test`: 184 pruebas Python y 54 TypeScript/JavaScript aprobadas.
- `pnpm build`: aprobado.
- `cargo check --offline --manifest-path src-tauri/Cargo.toml`: aprobado.
- `cargo test --offline --manifest-path src-tauri/Cargo.toml`: 581 aprobadas
  (502 biblioteca, 25 host y 54 integración).
- `cargo build --offline --manifest-path src-tauri/Cargo.toml --features native-default`:
  aprobado; ejecutable de desarrollo actualizado en
  `src-tauri/target/debug/linux-scrollmaster`.
- Formato Rust y `git diff --check`: aprobados; código modificado por debajo
  de 1000 líneas por archivo.

El sandbox impidió descargas y pruebas de sockets/procesos; los comandos completos
pasaron al ejecutarse con los permisos correspondientes. Las pruebas no requieren
mouse, root, hidraw ni compositor real.

El usuario confirmó el 26 de octubre que funciona bien tras la corrección y
autorizó publicar la release 0.2.1. Es aceptación del comportamiento reportada
por el usuario; no se midió la duración de inactividad ni el tiempo de las
consultas. No se modificaron perfiles ni configuraciones del sistema.
