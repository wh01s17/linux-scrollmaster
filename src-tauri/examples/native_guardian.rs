// SPDX-License-Identifier: GPL-3.0-or-later
//! Finite guardian. Capture requires explicit bindings and restores on termination.
use linux_scrollmaster::native::{
    run_guardian_child, ActionBindings, GuardianConfig, NativeGuardian, ProfileButtonPlan,
};
use serde_json::json;
use std::{process::ExitCode, time::Duration};

enum Invocation {
    Child(GuardianConfig),
    Client {
        config: GuardianConfig,
        refresh: bool,
    },
}
fn assignment_json(text: &str) -> Result<serde_json::Value, String> {
    if text.len() > 8192 {
        return Err("Capture assignments exceed 8192 bytes".into());
    }
    serde_json::from_str(text).map_err(|_| "Invalid assignment JSON".into())
}
fn bindings(text: &str) -> Result<serde_json::Value, String> {
    let value = assignment_json(text)?;
    ActionBindings::parse(&value)?;
    Ok(value)
}
fn parse(args: &[String]) -> Result<Invocation, String> {
    if (7..=8).contains(&args.len()) && args[0] == "--guardian-child" {
        let controls = args[4]
            .split(',')
            .map(str::parse)
            .collect::<Result<Vec<u16>, _>>()
            .map_err(|_| "Invalid recovery controls")?;
        let mut config = GuardianConfig::new(
            &args[1],
            args[2].parse().map_err(|_| "Invalid slot")?,
            &args[3],
            &controls,
            Duration::from_millis(args[5].parse().map_err(|_| "Invalid duration")?),
        )?;
        match (args[6].as_str(), args.len()) {
            ("keyboard-check", 7) => config = config.with_keyboard_check(),
            ("none", 7) => (),
            ("capture", 8) => config = config.with_capture_actions(&bindings(&args[7])?)?,
            ("capture-profile", 8) => {
                config = config.with_profile_buttons(&assignment_json(&args[7])?)?
            }
            _ => return Err("Invalid guardian keyboard mode".into()),
        }
        return Ok(Invocation::Child(config));
    }
    if !(8..=11).contains(&args.len())
        || !matches!(args[0].as_str(), "--observe" | "--capture")
        || args[2] != "--slot"
        || args[4] != "--device"
        || args[6] != "--seconds"
    {
        return Err("Uso: native_guardian --observe hidrawN --slot 1..6 --device UNITID --seconds 1..30 [--refresh] [--keyboard-check]; o --capture con --bindings-json OBJETO o --profile-buttons-json OBJETO [--refresh]. Capture activa temporalmente diversion/raw XY y emite acciones; restaura al cerrar. Profile-buttons exige remapeos nativos ya aplicados; no aplica ajustes ni guarda perfiles.".into());
    }
    let (mut refresh, mut keyboard) = (false, false);
    let mut assignments = None;
    let mut profile = false;
    let mut flags = args[8..].iter();
    while let Some(flag) = flags.next() {
        match flag.as_str() {
            "--refresh" if !refresh => refresh = true,
            "--keyboard-check" if !keyboard => keyboard = true,
            "--bindings-json" if assignments.is_none() => {
                assignments = Some(bindings(flags.next().ok_or("Missing assignment JSON")?)?)
            }
            "--profile-buttons-json" if assignments.is_none() => {
                let value = assignment_json(flags.next().ok_or("Missing profile buttons JSON")?)?;
                ProfileButtonPlan::parse(&value)?;
                assignments = Some(value);
                profile = true;
            }
            _ => return Err("Unknown or duplicate guardian flag".into()),
        }
    }
    if (args[0] == "--capture") != assignments.is_some() || (assignments.is_some() && keyboard) {
        return Err("Capture requires explicit assignments and cannot use keyboard-check; observe cannot have assignments".into());
    }
    let seconds = args[7].parse::<u64>().map_err(|_| "Invalid duration")?;
    if !(1..=30).contains(&seconds) {
        return Err("Choose 1..30 seconds".into());
    }
    let controls = assignments
        .as_ref()
        .map(|v| {
            if profile {
                ProfileButtonPlan::parse(v).map(|p| p.capture_controls())
            } else {
                ActionBindings::parse(v).map(|a| a.controls())
            }
        })
        .transpose()?
        .unwrap_or_else(|| vec![83, 86, 195]);
    let mut config = GuardianConfig::new(
        &args[1],
        args[3].parse().map_err(|_| "Invalid slot")?,
        &args[5],
        &controls,
        Duration::from_secs(seconds),
    )?;
    if keyboard {
        config = config.with_keyboard_check();
    }
    if let Some(assignments) = &assignments {
        config = if profile {
            config.with_profile_buttons(assignments)?
        } else {
            config.with_capture_actions(assignments)?
        };
    }
    Ok(Invocation::Client { config, refresh })
}
fn run(args: &[String]) -> Result<serde_json::Value, String> {
    let (config, refresh) = match parse(args)? {
        Invocation::Child(config) => {
            let _ = run_guardian_child(config)?;
            return Ok(json!({"child":true}));
        }
        Invocation::Client { config, refresh } => (config, refresh),
    };
    let mut client = NativeGuardian::spawn(config)?;
    if refresh {
        // A finite owner can close during refresh. finish still preserves the
        // terminal snapshot/recovery report instead of dropping that evidence.
        let _ = client.refresh(Duration::from_secs(20));
    }
    loop {
        std::thread::sleep(Duration::from_millis(100));
        if client.status().is_err() {
            break;
        }
    }
    let data = client.finish()?;
    Ok(json!({"ok":data["completed"],"data":data}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(mode: &str, tail: &[&str]) -> Vec<String> {
        [
            vec![
                mode,
                "hidraw0",
                "--slot",
                "1",
                "--device",
                "11223344",
                "--seconds",
                "1",
            ],
            tail.to_vec(),
        ]
        .concat()
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
    #[test]
    fn observe_and_capture_are_distinct_explicit_modes() {
        assert!(matches!(
            parse(&args("--observe", &["--keyboard-check", "--refresh"])).unwrap(),
            Invocation::Client { refresh: true, .. }
        ));
        assert!(matches!(
            parse(&args(
                "--capture",
                &["--bindings-json", r#"{"83":"shortcut:copy"}"#]
            ))
            .unwrap(),
            Invocation::Client { refresh: false, .. }
        ));
    }
    #[test]
    fn capture_never_uses_empty_or_implicit_assignments() {
        for tail in [
            vec![],
            vec!["--bindings-json"],
            vec!["--bindings-json", "{}"],
            vec!["--bindings-json", "null"],
            vec!["--bindings-json", r#"{"83":"shell:echo hi"}"#],
            vec!["--bindings-json", r#"{"80":"modifier:ctrl"}"#],
        ] {
            assert!(parse(&args("--capture", &tail)).is_err());
        }
    }
    #[test]
    fn assignments_cannot_be_smuggled_into_observe_or_keyboard_check() {
        let json = r#"{"83":"modifier:alt"}"#;
        assert!(parse(&args("--observe", &["--bindings-json", json])).is_err());
        assert!(parse(&args(
            "--capture",
            &["--keyboard-check", "--bindings-json", json]
        ))
        .is_err());
        assert!(parse(&args("--observe", &["--refresh", "--refresh"])).is_err());
    }
    #[test]
    fn startup_json_is_bounded_before_parsing() {
        assert!(bindings(&" ".repeat(8193)).unwrap_err().contains("8192"));
        assert!(bindings("{").is_err());
    }
    #[test]
    fn internal_child_requires_matching_registration_and_exact_mode_fields() {
        let mut child: Vec<String> = [
            "--guardian-child",
            "hidraw0",
            "1",
            "11223344",
            "83",
            "1000",
            "capture",
            r#"{"83":"modifier:ctrl"}"#,
        ]
        .map(str::to_owned)
        .to_vec();
        assert!(matches!(parse(&child).unwrap(), Invocation::Child(_)));
        child[4] = "83,86".into();
        assert!(parse(&child).is_err());
        child[6] = "none".into();
        assert!(parse(&child).is_err());
    }
    #[test]
    fn malformed_commands_are_rejected_without_running_a_guardian() {
        assert!(parse(&[]).is_err());
        let mut input = args(
            "--capture",
            &["--bindings-json", r#"{"83":"modifier:ctrl"}"#],
        );
        input[7] = "31".into();
        assert!(parse(&input).is_err());
        input[7] = "1".into();
        input[3] = "7".into();
        assert!(parse(&input).is_err());
    }
    #[test]
    fn profile_buttons_accept_mixed_assignments_and_reject_native_only_capture() {
        assert!(matches!(
            parse(&args(
                "--capture",
                &[
                    "--profile-buttons-json",
                    r#"{"83":86,"86":"shortcut:copy"}"#
                ]
            ))
            .unwrap(),
            Invocation::Client { .. }
        ));
        for value in [
            r#"{"83":86}"#,
            "{}",
            r#"{"83":"media:volume-up"}"#,
            r#"{"80":"modifier:ctrl"}"#,
        ] {
            assert!(parse(&args("--capture", &["--profile-buttons-json", value])).is_err());
        }
    }
    #[test]
    fn profile_buttons_never_enter_observe_or_mix_with_other_sources() {
        let value = r#"{"83":"modifier:ctrl"}"#;
        for (mode, tail) in [
            ("--observe", vec!["--profile-buttons-json", value]),
            (
                "--capture",
                vec!["--profile-buttons-json", value, "--keyboard-check"],
            ),
            (
                "--capture",
                vec![
                    "--profile-buttons-json",
                    value,
                    "--profile-buttons-json",
                    value,
                ],
            ),
            (
                "--capture",
                vec!["--bindings-json", value, "--profile-buttons-json", value],
            ),
        ] {
            assert!(parse(&args(mode, &tail)).is_err());
        }
    }
    #[test]
    fn child_profile_mode_revalidates_the_exact_software_recovery_set() {
        let mut child: Vec<String> = [
            "--guardian-child",
            "hidraw0",
            "1",
            "11223344",
            "86",
            "1000",
            "capture-profile",
            r#"{"83":86,"86":"shortcut:copy"}"#,
        ]
        .map(str::to_owned)
        .to_vec();
        assert!(matches!(parse(&child).unwrap(), Invocation::Child(_)));
        child[4] = "83,86".into();
        assert!(parse(&child).is_err());
        child[4] = "83".into();
        assert!(parse(&child).is_err());
    }
}
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = run(&args).unwrap_or_else(|error| json!({"ok":false,"error":error}));
    if result["child"] == true {
        return ExitCode::SUCCESS;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&result).expect("JSON value serializes")
    );
    if result["ok"] == true {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
