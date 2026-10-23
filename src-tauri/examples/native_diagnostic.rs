// SPDX-License-Identifier: GPL-3.0-or-later
//! Opt-in tool, never launched by the GUI. Default mode only reads sysfs.
use linux_scrollmaster::native::{
    diagnose, discover, ensure_no_controllers, paired_devices, run_bounded, HardwareLease,
};
use serde_json::{json, Value};
use std::{
    process::{Command, ExitCode},
    time::{Duration, Instant},
};

fn run(args: &[String]) -> Result<Value, String> {
    if args.is_empty() || args == ["--list"] {
        let found = discover()?;
        return Ok(
            json!({"ok":true,"mode":"passive-discovery","candidates":found.candidates.iter().map(|c|c.describe()).collect::<Vec<_>>(),"warnings":found.warnings}),
        );
    }
    let child = args.first().is_some_and(|arg| arg == "--worker");
    let args = if child { &args[1..] } else { args };
    let pairing = args.len() == 2 && args[0] == "--paired";
    if !pairing && (args.len() != 4 || args[0] != "--read" || args[2] != "--slot") {
        return Err(
            "Uso: native_diagnostic [--list | --paired hidrawN | --read hidrawN --slot 1..6]"
                .into(),
        );
    }
    let slot: u8 = if pairing {
        1
    } else {
        args[3].parse().map_err(|_| "Slot inválido")?
    };
    if !(1..=6).contains(&slot) {
        return Err("Selecciona un slot Bolt entre 1 y 6".into());
    }
    // Selection is always from discovery, never an arbitrary path.
    let found = discover()?;
    let candidate = found
        .candidates
        .iter()
        .find(|c| c.name() == args[1])
        .ok_or("La interfaz no está en el catálogo Bolt HID++ compatible")?;
    ensure_no_controllers()?;
    if !child {
        let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        command.arg("--worker").args(args);
        return run_bounded(command, Duration::from_secs(25));
    }
    let _lease = HardwareLease::acquire()?;
    ensure_no_controllers()?;
    let started = Instant::now();
    let io = candidate.open()?;
    let mut data = if pairing {
        paired_devices(io, started + Duration::from_secs(20))?
    } else {
        diagnose(io, slot, started + Duration::from_secs(20))?
    };
    data["interface"] = candidate.describe();
    data["elapsed_ms"] = json!(started.elapsed().as_millis());
    Ok(json!({"ok":true,"data":data}))
}
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let reply = run(&args).unwrap_or_else(|error| json!({"ok":false,"error":error}));
    println!(
        "{}",
        serde_json::to_string_pretty(&reply).expect("JSON value serializes")
    );
    if reply["ok"] == true {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
