// SPDX-License-Identifier: GPL-3.0-or-later
//! Opt-in, finite, passive notification observation. No setters or actions.
use linux_scrollmaster::native::{
    discover, ensure_no_controllers, observe_notifications, run_bounded, HardwareLease,
    PassiveEventOwner,
};
use serde_json::{json, Value};
use std::{
    process::{Command, ExitCode},
    time::Duration,
};

fn run(args: &[String]) -> Result<Value, String> {
    let worker = args.first().is_some_and(|s| s == "--worker");
    let args = if worker { &args[1..] } else { args };
    let refresh = args.len() == 9 && args[8] == "--refresh";
    if (args.len() != 8 && !refresh)
        || args[0] != "--observe"
        || args[2] != "--slot"
        || args[4] != "--device"
        || args[6] != "--seconds"
    {
        return Err("Uso: native_events --observe hidrawN --slot 1..6 --device UNITID --seconds 1..30 [--refresh]. Solo lectura; no habilita diversion.".into());
    }
    let slot: u8 = args[3].parse().map_err(|_| "Invalid slot")?;
    let seconds: u64 = args[7]
        .parse()
        .map_err(|_| "Invalid observation duration")?;
    let id = &args[5];
    if !(1..=6).contains(&slot)
        || !(1..=30).contains(&seconds)
        || id.len() != 8
        || !id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())
    {
        return Err("Expected Bolt slot, uppercase Unit ID and 1..30 seconds".into());
    }
    ensure_no_controllers()?;
    let found = discover()?;
    let candidate = found
        .candidates
        .iter()
        .find(|c| c.name() == args[1])
        .ok_or("Interface not in the supported Bolt catalogue")?;
    if !worker {
        let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        command.arg("--worker").args(args);
        return run_bounded(command, Duration::from_secs(25 + seconds));
    }
    let _lease = HardwareLease::acquire()?;
    ensure_no_controllers()?;
    let data = if refresh {
        let mut owner = PassiveEventOwner::open(
            candidate.open()?,
            slot,
            id,
            Duration::from_secs(seconds),
            || false,
        )?;
        // One complete fixed-catalogue query while the same consumer is installed.
        let _ = owner.refresh(Duration::from_secs(20));
        while owner.is_running() {
            let _ = owner.poll(Duration::from_millis(50));
        }
        owner.status()
    } else {
        observe_notifications(
            candidate.open()?,
            slot,
            id,
            Duration::from_secs(seconds),
            || false,
        )?
    };
    // Partial evidence is preserved on stream faults and limits, never declared success.
    if data["observation"]["completed"] != true {
        return Ok(json!({"ok":false,"error":"Passive observation incomplete","data":data}));
    }
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
