# Empaquetado Arch (preparación, no publicación)

La receta usa un tarball local linux-scrollmaster-0.1.0.tar.gz con carpeta raíz linux-scrollmaster-0.1.0.
Una vez revisado y versionado el código, genera una exportación limpia de los
archivos mantenidos; nunca uses `tar` indiscriminadamente sobre el workspace.

Para generar un kit local desde un commit limpio, con checksum real y manifiesto:

```sh
python packaging/arch/prepare.py /tmp/linux-scrollmaster-arch-kit
```

El script exporta solo archivos versionados, rechaza binarios propietarios y
directorios privados, conserva el commit completo y genera gzip determinista con
SHA-256 real en el PKGBUILD de salida. No sobrescribe kits existentes. El PKGBUILD
del repositorio conserva SKIP como plantilla; el kit generado no lo usa.

En el directorio del kit usa `makepkg` como
usuario normal. El build necesita red para dependencias pnpm/Cargo. No ejecutar
makepkg como root. La dependencia Solaar queda fijada a 1.1.20; no se elimina esa restricción para
aceptar versiones no probadas. Antes de publicar, definir versión/fuente pública,
probar en chroot limpio, revisar licencias transitivas y verificar ruta de recursos
Tauri `/usr/lib/linux-scrollmaster/bridge/service.py` en la instalación final.

No se instaló ni publicó este paquete durante el desarrollo inicial. Reutiliza
reglas udev del paquete Solaar; no incluye reglas globales de acceso a dispositivos.
AppImage/Flatpak pendientes: acceso hidraw, Python del sistema y reglas del host
necesitan una estrategia explícita; no prometer portabilidad autosuficiente todavía.

Validación de desarrollo: build release offline completado; kit bf47aa9 generado
dos veces con SHA idéntico y `makepkg --verifysource` aprobado. No equivale a un
paquete instalado o un chroot limpio. La ruta Tauri Linux se contrastó con
`tauri-utils::platform::resource_dir_from`: usa ../lib/linux-scrollmaster si existe, coherente
con la receta. Aceptación de ejecución instalada todavía pendiente.

Variante nativa (N6): `python packaging/arch/prepare.py --variant native <dir>` usa
`packaging/arch-native/PKGBUILD`, sin Python ni Solaar, con la regla
`packaging/udev/70-linux-scrollmaster.rules` (solo interfaz HID++ 02 del receptor
Bolt). Pendiente de makepkg e instalación limpia; ver docs/native-acceptance.md.
