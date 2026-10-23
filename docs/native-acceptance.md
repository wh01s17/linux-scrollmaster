# Protocolo de aceptación física del backend nativo (N3–N6)

Guía para cerrar el Hito N con el mouse real. **Todas estas pruebas son optativas
y modifican temporalmente el hardware.** Las ejecuta la persona usuaria; el
desarrollo automático no abre el mouse. Ninguna casilla se marca sin el resultado
observado. Equipo de referencia: receptor Bolt en `hidraw7`, slot 2, Unit ID
`<UNIT_ID>` (confirmar con el paso 0, puede cambiar).

## Alcance y seguridad

- Cada bloque anota los valores originales antes y los restaura al final, también
  si algo falla. El paso 0 sirve de referencia para comprobarlo.
- Cerrar Solaar, logid y cualquier otra instancia de Linux ScrollMaster: el backend
  nativo se niega a abrir el receptor si los detecta. No se detienen procesos por ti.
- No ejecutar como root. Hasta instalar la regla propia, el acceso viene de la
  regla udev de Solaar ya presente en el sistema.
- Si una captura queda activa por un fallo (botón sin función), cerrar la app o
  matar el worker: el guardián restaura. Comprobar con el paso 0.

## 0. Referencia de solo lectura (antes y después de cada bloque)

```sh
pgrep -a solaar; pgrep -a logid    # ambos deben estar vacíos
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_diagnostic -- --read hidraw7 --slot 2
cargo run --offline --manifest-path src-tauri/Cargo.toml --example native_buttons -- --read hidraw7 --slot 2 --device <UNIT_ID>
```

Anotar los seis ajustes y, para 83/86/195, destino y `flags`. Al terminar cada
bloque, los `flags` deben ser 0 y los valores iguales a los originales.

## 1. Arranque nativo desde desarrollo

```sh
SCROLLMASTER_BACKEND=native pnpm tauri dev
```

- [x] La app localiza el mouse sin Solaar, muestra batería, protocolo y seis ajustes.
- [x] Con el mouse apagado: aparece el diagnóstico y los perfiles siguen visibles;
      renombrar un perfil funciona offline. Al encenderlo vuelve en ≤15 s.

## 2. Ajustes y remapeo nativos (aceptación ampliada N3)

- [x] Cambiar y restaurar cada uno de los seis ajustes; la UI confirma lectura.
- [x] Atrás→Adelante→Atrás y gestos→clic central→gestos; restaurar originales.

## 3. Captura y acciones (N4)

- [x] Atrás = Ctrl mantenido: Ctrl+rueda hace zoom; al soltar no queda Ctrl pegado.
- [x] Adelante = Copiar/Pegar y un atajo propio (por ejemplo Ctrl+Shift+T). Copiar aceptado; **atajo propio sin prueba física**.
- [x] Acción Hyprland (espacio siguiente) y multimedia desde un gesto.
- [x] Gestos: cuatro direcciones y clic, con distancia estable al cambiar DPI.
- [x] Volver Atrás a su función nativa: `flags` 0 en el paso 0.

## 4. Transiciones y foco (N5)

- [x] Perfil general con Atrás=Ctrl y regla para una app con Atrás=Alt: alternar
      foco cambia el modificador sin reiniciar la app; volver al general restaura Ctrl.
- [x] Regla con Atrás nativo: al enfocar esa app el botón vuelve a Atrás nativo.
- [x] `remap` de un solo botón conserva la captura del otro.

## 5. Recuperación

- [x] Con Ctrl mantenido, `pkill -9 -f -- --serve-guarded-capture`: Ctrl se libera y
      `flags` vuelven a 0; el siguiente sondeo reconecta.
- [x] Apagar el mouse con captura activa y encenderlo: se recupera o se informa un
      error; si queda capturado, anotarlo (no hay garantía si el dispositivo es inaccesible).
- [x] Suspender y reanudar el equipo con captura activa.
- [x] Cerrar la app con captura activa: `flags` 0 al terminar.

## 6. Instalación limpia sin Solaar (N6)

```sh
git status --short   # vacío: el kit sale de un commit
python packaging/arch/prepare.py --variant native /tmp/scrollmaster-native-kit
cd /tmp/scrollmaster-native-kit && makepkg -f    # usuario normal, sin instalar
sudo pacman -U linux-scrollmaster-native-0.1.0-1-x86_64.pkg.tar.zst
```

`prepare.py` excluye del archivo de fuentes las herramientas de agentes versionadas
(`.agents`, `.claude`, `skills-lock.json`) y sigue abortando ante cualquier otro
archivo privado o propietario. Si `nodejs`/`pnpm` vienen de mise/nvm y no de
pacman, `makepkg -d` omite solo esa comprobación de dependencias de compilación.

Build verificado el 22 de octubre de 2026 desde el commit `159af2e` (sin instalar):
release en 4 min 43 s; el paquete contiene solo binario, regla udev, `.desktop`,
icono y licencia; 0 archivos Python/Solaar; dependencias solo de sistema; compilado
con `--features native-default`; el binario empaquetado rechaza argumentos de
worker inválidos sin abrir hardware.

- [ ] Desinstalar Solaar (o comprobar que no está instalado) y reconectar el receptor.
- [ ] `getfacl /dev/hidraw7` muestra tu usuario solo en la interfaz 02; las 00/01 no.
- [ ] `linux-scrollmaster` funciona sin `SCROLLMASTER_BACKEND` y sin Python en runtime
      (`pgrep -a python` vacío durante el uso).
- [ ] Repetir 1–5 sobre la instalación.
- [ ] Desinstalar: `~/.config/linux-scrollmaster/profiles.json` intacto.

Bloques 1–5 aceptados el 22 de octubre de 2026 (detalle y defectos corregidos
en `plan.md`, «Aceptación física guiada»). Multimedia (volumen) y espacios por gesto verificados; el atajo propio
no se probó físicamente.

Registrar en `plan.md` fecha, versión, resultado por casilla y valores restaurados.
Tras aceptar 1–6, cambiar el default a nativo y retirar `bridge/` del paquete heredado.
