// SPDX-License-Identifier: GPL-3.0-or-later
#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    let args = std::env::args().skip(1).collect();
    match linux_scrollmaster::worker::main_with_args(args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Native worker requires Linux");
    std::process::exit(1);
}
