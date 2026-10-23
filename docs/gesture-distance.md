# Distancia de gestos estable al cambiar DPI

## Implementación

El editor de gestos incorpora «Conservar la distancia al cambiar DPI» y un rango
estimado de 1–20 mm, con pasos de 0,1 mm. Un gesto nuevo propone 3 mm. Las
asignaciones existentes conservan su umbral en cuentas del sensor hasta que el
usuario convierta expresamente la unidad y pulse Aplicar. La conversión aproxima
el recorrido actual usando los DPI leídos y limita el resultado al rango editable.
No modifica perfiles automáticamente.

El objeto añade `units: 'tenths-mm'`, con `threshold` entero en décimas de mm.
Los perfiles que guardan unidades explícitas usan schema 8. Siguen aceptándose
schemas 1–7 y gestos sin `units`, que conservan cuentas crudas (20–2000).
`units: 'counts'` también es válido en schema 8. Guardar, restaurar, reconciliar,
importar y exportar conservan la unidad sin migración implícita.

El motor calcula `ceil(threshold * DPI / 254)` cuentas, acumula enteros y conserva
la clasificación estabilizada anterior. El adaptador lee DPI sin caché antes de
capturar el botón, actualiza la escala tras una escritura confirmada y contrasta
los DPI en los sondeos habituales. No añade consultas por paquete de movimiento.
Un cambio de DPI durante una pulsación cancela el gesto y exige soltar para
rearmar. Si se pierde la lectura DPI durante captura física, se detiene el consumidor
y se restituyen los botones; el error pide volver a aplicar.

Son distancias estimadas mediante la resolución anunciada por el sensor, no una
calibración con regla ni una reproducción del algoritmo de Windows. Un cambio
externo de DPI se detecta en el siguiente sondeo, no instantáneamente.

## Verificación del incremento

Suite completa: **178 pruebas** (143 Python, 17 estado Vue, 5 conversiones del
editor y 13 Rust). Once regresiones Python nuevas comprueban alcance equivalente
a distintos DPI, redondeo, cancelación y supresión hasta liberar, validación,
lectura sin caché, pérdida de DPI y persistencia/restauración/reconciliación con
dobles. Las cinco pruebas TypeScript cubren conversión explícita, límites,
formato y ausencia de mutaciones. `pnpm build`, `cargo check --offline`,
`cargo test --offline` y ejecutable debug integrado completaron correctamente.

Prueba optativa real autorizada, con Linux ScrollMaster cerrada:

```sh
python tests/hardware_modifiers_roundtrip.py --physical-distance
```

Aprobó en MX Master 3S <UNIT_ID> por Bolt: configuración física con acciones neutras,
escritura y lectura de DPI diferentes, flags reales de captura y restitución
tras cierre normal y SIGKILL del worker. El bloque finally restauró y confirmó
los DPI originales; perfiles intactos y ninguna tecla sintetizada por esta prueba.
La lectura independiente posterior (2026-10-18 04:23:20 UTC) confirmó 1850 DPI,
botones nativos y perfil schema 7 intacto, sin reaplicación activa.

Pendiente: comparar físicamente el recorrido desde GUI a 1850 y 3700 DPI y
aceptar el nuevo selector. También queda comprobar guardar/restaurar/reconectar
un perfil schema 8 con el dispositivo real; las pruebas de persistencia actuales
usan dobles y no sustituyen ese ciclo.
