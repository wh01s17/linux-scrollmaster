// SPDX-License-Identifier: GPL-3.0-or-later
//! N3 opt-in mutation tool. Never invoked by GUI or regular tests.
use linux_scrollmaster::native::{
    apply_changes, diagnose, discover, ensure_no_controllers, run_bounded_settings,
    validate_changes, Candidate, HardwareLease,
};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    process::{Command, ExitCode},
    time::{Duration, Instant},
};

fn roundtrip(candidate: &Candidate, slot: u8, id: &str) -> Result<Value, String> {
    let before = diagnose(
        candidate.open()?,
        slot,
        Instant::now() + Duration::from_secs(10),
    )?;
    if before["device"]["id"] != id {
        return Err("Device identity differs; roundtrip not started".into());
    }
    let settings = &before["device"]["settings"];
    if !settings
        .as_object()
        .is_some_and(|s| s.len() == 6 && s.values().all(|v| v["available"] == true))
    {
        return Err("Six complete baseline readings required before roundtrip".into());
    }
    let dpi = settings["dpi"]["value"]
        .as_u64()
        .ok_or("Missing current DPI")?;
    let choices = settings["dpi"]["choices"]
        .as_array()
        .ok_or("Missing DPI choices")?;
    let position = choices
        .iter()
        .position(|c| c == dpi)
        .ok_or("Current DPI not advertised")?;
    let temporary = choices
        .get(position + 1)
        .or_else(|| position.checked_sub(1).and_then(|p| choices.get(p)))
        .ok_or("No neighboring DPI choice")?
        .clone();
    // Recovery record survives an unexpected parent/worker termination. Never a profile.
    let recovery = std::env::temp_dir().join(format!(
        "scrollmaster-n3-recovery-{}.json",
        std::process::id()
    ));
    let mut journal = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&recovery)
        .map_err(|e| e.to_string())?;
    journal.write_all(serde_json::to_vec_pretty(&json!({"device":id,"slot":slot,"interface":candidate.name(),"restore":{"dpi":dpi},"baseline":settings})).map_err(|e|e.to_string())?.as_slice()).and_then(|_|journal.sync_all()).map_err(|e|e.to_string())?;
    drop(journal);
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ensure_no_controllers()?;
        apply_changes(
            candidate.open()?,
            slot,
            id,
            &json!({"dpi":temporary}),
            Instant::now() + Duration::from_secs(20),
        )
        .map_err(|e| e.to_string())
    }))
    .unwrap_or_else(|_| Err("Roundtrip attempt panicked".into()));
    // Finally: always reread identity/capabilities before restoring the original DPI.
    let restored = (|| {
        ensure_no_controllers()?;
        apply_changes(
            candidate.open()?,
            slot,
            id,
            &json!({"dpi":dpi}),
            Instant::now() + Duration::from_secs(20),
        )
        .map_err(|e| e.to_string())
    })();
    let restored = restored.map_err(|e| {
        format!(
            "Restoration not confirmed: {e}. Recovery record: {}",
            recovery.display()
        )
    })?;
    let after = &restored["snapshot"]["device"]["settings"];
    if !settings.as_object().unwrap().iter().all(|(key, value)| {
        after[key]["available"] == true && after[key]["value"] == value["value"]
    }) {
        return Err(format!(
            "Final six settings differ from baseline; recovery record: {}",
            recovery.display()
        ));
    }
    fs::remove_file(&recovery)
        .map_err(|e| format!("Restored, but recovery record could not be removed: {e}"))?;
    let attempt = attempt.map_err(|e| {
        format!("Test failed; original six values confirmed after restoration: {e}")
    })?;
    Ok(
        json!({"stage":"N3","read_only":false,"mode":"dpi-roundtrip","device":id,"confirmed":attempt["confirmed"],"temporary_dpi":temporary,"original_dpi":dpi,"restored":true,"snapshot":restored["snapshot"]}),
    )
}

fn run(args: &[String]) -> Result<Value, String> {
    let child = args.first().is_some_and(|s| s == "--worker");
    let args = if child { &args[1..] } else { args };
    let roundtrip_mode = args.len() == 6 && args[0] == "--roundtrip";
    let apply = args.len() == 8 && args[0] == "--apply" && args[6] == "--changes";
    if (!roundtrip_mode && !apply)
        || args.get(2).map(String::as_str) != Some("--slot")
        || args.get(4).map(String::as_str) != Some("--device")
    {
        return Err("Uso: native_settings (--apply hidrawN --slot 1..6 --device UNITID --changes '{...}' | --roundtrip hidrawN --slot 1..6 --device UNITID). Esta herramienta modifica el hardware.".into());
    }
    let slot: u8 = args[3].parse().map_err(|_| "Invalid slot")?;
    if !(1..=6).contains(&slot) {
        return Err("Invalid Bolt slot".into());
    }
    let id = &args[5];
    if id.len() != 8
        || !id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())
    {
        return Err("Expected uppercase Unit ID".into());
    }
    let changes = if apply {
        if args[7].len() > 1024 {
            return Err("Settings input too large".into());
        }
        let value = serde_json::from_str(&args[7]).map_err(|_| "Invalid settings JSON")?;
        validate_changes(&value)?;
        value
    } else {
        Value::Null
    };
    ensure_no_controllers()?;
    let found = discover()?;
    let candidate = found
        .candidates
        .iter()
        .find(|c| c.name() == args[1])
        .ok_or("Interface not in the supported Bolt catalog")?;
    if !child {
        let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        command.arg("--worker").args(args);
        return run_bounded_settings(command, Duration::from_secs(60));
    }
    let _lease = HardwareLease::acquire()?;
    ensure_no_controllers()?;
    let data = if roundtrip_mode {
        roundtrip(candidate, slot, id)?
    } else {
        match apply_changes(
            candidate.open()?,
            slot,
            id,
            &changes,
            Instant::now() + Duration::from_secs(40),
        ) {
            Ok(data) => data,
            Err(error) => return Ok(error.describe()),
        }
    };
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
