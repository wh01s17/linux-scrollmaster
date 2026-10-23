fn main() {
    println!("cargo:rerun-if-changed=../native/keyboard.c");
    println!("cargo:rerun-if-changed=../native/wayland_wait.c");
    println!("cargo:rerun-if-changed=../native/wayland_wait.h");
    let flags = std::process::Command::new("pkg-config")
        .args(["--cflags", "--libs", "wayland-client", "xkbcommon"])
        .output()
        .expect("pkg-config para Wayland/xkbcommon");
    assert!(
        flags.status.success(),
        "Faltan bibliotecas de desarrollo Wayland/xkbcommon"
    );
    let flags = String::from_utf8(flags.stdout).expect("pkg-config UTF-8");
    // A running guardian may have the current library mapped. Never truncate it.
    let temporary = std::path::PathBuf::from(format!(
        "../bridge/.libscrollmaster-keyboard-{}.so",
        std::process::id()
    ));
    let status = std::process::Command::new("cc")
        .args([
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-shared",
            "-fPIC",
            "../native/keyboard.c",
            "../native/wayland_wait.c",
            "-o",
        ])
        .arg(&temporary)
        .args(flags.split_whitespace())
        .status()
        .expect("Compilador C para teclado Wayland");
    if !status.success() {
        let _ = std::fs::remove_file(&temporary);
        panic!("No se pudo compilar el cliente de teclado Wayland");
    }
    std::fs::rename(&temporary, "../bridge/libscrollmaster-keyboard.so")
        .expect("No se pudo publicar la biblioteca de teclado Wayland");
    // Link our C implementation into the native Rust backend; no runtime dlopen,
    // resource path or Python is required by the new keyboard wrapper.
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let mut objects = Vec::new();
    for source in ["keyboard", "wayland_wait"] {
        let object = out.join(format!("{source}.o"));
        let status = std::process::Command::new("cc")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-fPIC", "-c"])
            .arg(format!("../native/{source}.c"))
            .arg("-o")
            .arg(&object)
            .args(
                flags
                    .split_whitespace()
                    .filter(|flag| !flag.starts_with("-l") && !flag.starts_with("-L")),
            )
            .status()
            .expect("Compilador C para teclado nativo");
        assert!(status.success(), "No se pudo compilar {source}");
        objects.push(object);
    }
    let status = std::process::Command::new("ar")
        .arg("crs")
        .arg(out.join("libscrollmaster_keyboard_native.a"))
        .args(&objects)
        .status()
        .expect("Archivador C para teclado nativo");
    assert!(status.success(), "No se pudo archivar el teclado nativo");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=scrollmaster_keyboard_native");
    for flag in flags.split_whitespace() {
        if let Some(lib) = flag.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={lib}");
        }
        if let Some(path) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={path}");
        }
    }
    tauri_build::build()
}
