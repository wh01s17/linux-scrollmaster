// SPDX-License-Identifier: GPL-3.0-or-later
//! Isolated N3 native button tool. Read is explicit; writes require --apply/--roundtrip.
use linux_scrollmaster::native::{
    apply_button_changes, discover, ensure_no_controllers, read_buttons, run_bounded,
    run_bounded_settings, validate_button_changes, Candidate, HardwareLease,
};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    process::{Command, ExitCode},
    time::{Duration, Instant},
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(20)
}
fn mappings(snapshot: &Value) -> Result<Value, String> {
    let controls = snapshot["controls"]
        .as_array()
        .ok_or("No button controls")?;
    if controls.len() != 3 || controls.iter().any(|c| c["available"] != true) {
        return Err(
            "Roundtrip requires all three secondary controls to be readable and not intercepted"
                .into(),
        );
    }
    Ok(Value::Object(
        controls
            .iter()
            .map(|c| (c["id"].as_str().unwrap().into(), c["value"].clone()))
            .collect(),
    ))
}
fn same_baseline(before: &Value, after: &Value) -> Result<bool, String> {
    let settings = &before["snapshot"]["device"]["settings"];
    Ok(mappings(before)? == mappings(after)?
        && settings.as_object().is_some_and(|s| {
            s.len() == 6
                && s.iter().all(|(key, value)| {
                    value["available"] == true
                        && after["snapshot"]["device"]["settings"][key]["available"] == true
                        && after["snapshot"]["device"]["settings"][key]["value"] == value["value"]
                })
        }))
}
fn roundtrip(candidate: &Candidate, slot: u8, id: &str) -> Result<Value, String> {
    let before = read_buttons(candidate.open()?, slot, id, deadline())?;
    let original = mappings(&before)?;
    if !same_baseline(&before, &before)? {
        return Err("Incomplete scalar baseline before remap test".into());
    }
    let source = before["controls"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["id"] == "83"))
        .ok_or("Back control missing")?;
    // Only exchange the back button's native task with another side-button task.
    let target = [86u16, 83]
        .into_iter()
        .find(|&target| {
            source["value"] != target
                && source["choices"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|c| c["value"] == target))
        })
        .ok_or("No alternative side-button function announced")?;
    let recovery = std::env::temp_dir().join(format!(
        "scrollmaster-n3-button-recovery-{}.json",
        std::process::id()
    ));
    let mut journal = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&recovery)
        .map_err(|e| e.to_string())?;
    journal.write_all(serde_json::to_vec_pretty(&json!({"device":id,"slot":slot,"interface":candidate.name(),"restore":{"83":original["83"]},"baseline":before})).map_err(|e|e.to_string())?.as_slice()).and_then(|_|journal.sync_all()).map_err(|e|e.to_string())?;
    drop(journal);
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ensure_no_controllers()?;
        apply_button_changes(
            candidate.open()?,
            slot,
            id,
            &json!({"83":target}),
            deadline(),
        )
        .map_err(|e| e.to_string())
    }))
    .unwrap_or_else(|_| Err("Native button test panicked".into()));
    // Finally: restore only the source actually tested, after a fresh identity read.
    let restoration = (|| {
        ensure_no_controllers()?;
        apply_button_changes(
            candidate.open()?,
            slot,
            id,
            &json!({"83":original["83"]}),
            deadline(),
        )
        .map_err(|e| e.to_string())
    })();
    let restoration = restoration.map_err(|e| {
        format!(
            "Restoration not confirmed: {e}. Recovery record: {}",
            recovery.display()
        )
    })?;
    if !same_baseline(&before, &restoration["snapshot"])? {
        return Err(format!(
            "Final button/scalar values differ from baseline; recovery record: {}",
            recovery.display()
        ));
    }
    fs::remove_file(&recovery)
        .map_err(|e| format!("Restored; recovery record could not be removed: {e}"))?;
    let attempt = attempt.map_err(|e| {
        format!("Original mappings and six settings restored after failed test: {e}")
    })?;
    Ok(
        json!({"stage":"N3","read_only":false,"mode":"native-button-roundtrip","device":id,"confirmed":attempt["confirmed"],"source":"83","original_target":original["83"],"temporary_target":target,"restored":true,"snapshot":restoration["snapshot"]}),
    )
}
fn run(args: &[String]) -> Result<Value, String> {
    let worker = args.first().is_some_and(|s| s == "--worker");
    let args = if worker { &args[1..] } else { args };
    let read = args.len() == 6 && args[0] == "--read";
    let roundtrip_mode = args.len() == 6 && args[0] == "--roundtrip";
    let apply = args.len() == 8 && args[0] == "--apply" && args[6] == "--changes";
    if (!read && !roundtrip_mode && !apply)
        || args.get(2).map(String::as_str) != Some("--slot")
        || args.get(4).map(String::as_str) != Some("--device")
    {
        return Err("Uso: native_buttons (--read | --roundtrip | --apply) hidrawN --slot 1..6 --device UNITID [--changes '{\"83\":86}']. --apply modifica funciones de botones; --roundtrip prueba y restaura la función original.".into());
    }
    let slot: u8 = args[3].parse().map_err(|_| "Invalid slot")?;
    let id = &args[5];
    if !(1..=6).contains(&slot)
        || id.len() != 8
        || !id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())
    {
        return Err("Expected a Bolt slot and uppercase Unit ID".into());
    }
    let changes = if apply {
        if args[7].len() > 1024 {
            return Err("Assignments input too large".into());
        }
        let value = serde_json::from_str(&args[7]).map_err(|_| "Invalid assignments JSON")?;
        validate_button_changes(&value)?;
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
        .ok_or("Interface not in the supported Bolt catalogue")?;
    if !worker {
        let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        command.arg("--worker").args(args);
        return if read {
            run_bounded(command, Duration::from_secs(25))
        } else {
            run_bounded_settings(command, Duration::from_secs(60))
        };
    }
    let _lease = HardwareLease::acquire()?;
    ensure_no_controllers()?;
    let data = if read {
        read_buttons(candidate.open()?, slot, id, deadline())?
    } else if roundtrip_mode {
        roundtrip(candidate, slot, id)?
    } else {
        match apply_button_changes(
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
