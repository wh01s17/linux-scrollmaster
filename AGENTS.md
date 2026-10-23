# Linux ScrollMaster: instrucciones de trabajo

Aplicación Linux independiente para MX Master 3S. Lee README, plan.md y docs/architecture.md.
No ejecutar instaladores ni binarios propietarios; solo análisis estático selectivo.
No incluir EXE/DLL/MSI, recursos visuales o tipografías de Logitech en Git ni paquetes.
No modificar configuración de Solaar, Hyprland, Omarchy ni servicios del usuario.
No ejecutar la interfaz como root ni conceder permisos globales de entrada.

## Codebase Memory

Preferir MCP graph para descubrir código: list_projects/index_status al iniciar,
search_graph, trace_path, get_code_snippet y check_index_coverage para cada ruta usada.
Nivel por defecto Verify. Paginar resultados relevantes. Una cobertura sin huecos
registrados no prueba exhaustividad. Ante huecos, exclusiones o frescura desconocida,
leer el código/rangos señalados. Para literales y configuración usar rg.
No delegar sin petición o instrucción aplicable y sin transmitir evidencia y alcance.

## Cambios y pruebas

- Mantener los archivos de código en un máximo de 1000 líneas; dividir por
  responsabilidad al acercarse al límite. Documentar ruta y justificación concreta
  de cualquier excepción, sin usarla como sustituto de una separación razonable.
- Mantener separado estado Vue, frontera Tauri, servicio, backend y perfiles.
- Validar capacidades reales antes de escribir y confirmar por lectura sin caché.
- No habilitar diversion sin un consumidor de eventos y restauración fiable.
- Nunca presentar mocks como hardware verificado. Marcar estados pendientes en UI/docs.
- `pnpm test`, `pnpm build`, `cargo check --offline --manifest-path src-tauri/Cargo.toml` y
  `cargo test --offline --manifest-path src-tauri/Cargo.toml` para cambios del supervisor.
- Pruebas físicas son optativas y mutan temporalmente el hardware: explicar alcance,
  comprobar ausencia de Solaar/logid y restaurar valores originales en finally.
- Las pruebas regulares no requieren mouse, root, red ni permisos hidraw.
- Registrar decisiones, evidencia, limitaciones y próximos pasos en docs.
