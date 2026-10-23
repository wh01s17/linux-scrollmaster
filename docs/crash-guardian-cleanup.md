# Limpieza de fixtures tras un fallo de prueba

22 de octubre de 2026, America/Santiago.

Los cores de los PID 1333609 y 1333646 corresponden a subprocesos Rust de
`guardian_fixture_child`, modos sintéticos `keyboard-events` y `capture-normal`.
Contienen un error ENOENT de `unwrap()` y un panic en un destructor durante otro
panic. La pila registrada pasa por `FixtureKeyboard` y la salida de
`serve_with_keyboard`. La hipótesis más probable es un timeout del padre seguido
de eliminación del sandbox mientras el hijo seguía usando `wire.log`.
El binario fue recompilado después; no se afirma conocer la línea original exacta.

## Cambio

Los clientes sintéticos lanzados manualmente quedan bajo un guard RAII que los
termina y recoge incluso durante unwind. El préstamo del sandbox exige ese orden.
No se mata al guardián: recibe EOF y conserva su restauración normal.
El sandbox espera hasta 25 segundos la publicación atómica de `result.json`, que
ocurre después de liberar teclado/transporte y restaurar. Se marca la espera
antes de lanzar el proceso para cubrir fallos tempranos. Si falta el informe al
vencer el plazo, se conserva el directorio y se indica la ruta por stderr.

Los logs de fixtures son de mejor esfuerzo y no provocan panic al fallar E/S.
Las pruebas siguen exigiendo los eventos esperados en `wire.log`. El destructor
del teclado comprueba cierre en salida normal, sin disparar otro panic durante
unwind. Los helpers se separan en `guardian_fixture_support.rs` para mantener el
archivo principal por debajo del límite de 1000 líneas.

## Validación y límites

Se añaden regresiones de fallo del padre con cliente vivo, preservación del
sandbox pendiente y pérdida del directorio durante unwind de mouse, teclado y
motor. El último caso corre en un subproceso para detectar SIGABRT sin derribar
la suite. Todas usan dispositivos sintéticos; no constituyen aceptación física.

Validación realizada:

- Suite del guardián: 35 pruebas aprobadas, incluidas las tres regresiones nuevas.
- `cargo check --offline --manifest-path src-tauri/Cargo.toml`: aprobado.
- `cargo test --offline --manifest-path src-tauri/Cargo.toml`: suite completa aprobada.
- Scripts `test` y `build` de package.json: aprobados mediante `npm run test` y
  `npm run build`. `pnpm` no está instalado en esta sesión; no se instalaron paquetes.
- Formato de los dos archivos Rust modificados y `git diff --check`: aprobados.

Próximo paso: si reaparece un fallo, conservar la salida de la prueba padre y el
sandbox cuya ruta se informa. La protección conserva evidencia en vez de borrar
recursos de un guardián cuyo informe final no llegó dentro del plazo.
La corrección previa del informe inicial de gesto se conserva sin cambios.
