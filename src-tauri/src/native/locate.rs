// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only selection of the single supported mouse behind one Bolt receiver.
//! Pairing metadata and the N2 diagnostic only: no setter, pairing or reporting.
use super::{diagnose, discover, ensure_no_controllers, paired_devices, HardwareLease};
use serde_json::Value;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub slot: u8,
    pub identity: String,
}

/// Takes the shared lease only for these reads; the caller must not hold
/// another reader. Ambiguity (receivers or mice) is an error, never a guess.
pub fn locate(timeout: Duration) -> Result<Located, String> {
    ensure_no_controllers()?;
    let _lease = HardwareLease::acquire()?;
    let deadline = Instant::now() + timeout;
    let discovery = discover()?;
    let candidate = match discovery.candidates.as_slice() {
        [candidate] => candidate,
        [] if discovery.warnings.is_empty() => {
            return Err("No se encontró un receptor Logi Bolt compatible.".into())
        }
        [] => {
            return Err(format!(
                "Sin acceso al receptor Logi Bolt: {}",
                discovery.warnings.join("; ")
            ))
        }
        _ => return Err("Hay varios receptores Logi Bolt; conecta solo uno.".into()),
    };
    select(
        || paired_devices(candidate.open()?, deadline),
        |slot| {
            let report = diagnose(candidate.open()?, slot, deadline)?;
            report["device"]["id"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "Diagnostic without Unit ID".into())
        },
    )
}

pub(super) fn select(
    pairing: impl FnOnce() -> Result<Value, String>,
    mut identify: impl FnMut(u8) -> Result<String, String>,
) -> Result<Located, String> {
    let pairing = pairing()?;
    let mut found = Vec::new();
    let mut errors = Vec::new();
    for slot in pairing["slots"].as_array().into_iter().flatten() {
        if slot["supported_model"] != true {
            continue;
        }
        let slot = slot["slot"]
            .as_u64()
            .and_then(|s| u8::try_from(s).ok())
            .filter(|s| (1..=6).contains(s))
            .ok_or("Invalid pairing slot")?;
        match identify(slot) {
            Ok(identity) => found.push(Located { slot, identity }),
            Err(error) => errors.push(format!("slot {slot}: {error}")),
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 if errors.is_empty() => {
            Err("No hay un MX Master 3S emparejado con el receptor Logi Bolt.".into())
        }
        0 => Err(format!(
            "No se encontró un MX Master 3S activo. Enciéndelo o muévelo. ({})",
            errors.join("; ")
        )),
        _ => Err("Hay varios MX Master 3S activos; deja solo uno encendido.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pairing(slots: Value) -> impl FnOnce() -> Result<Value, String> {
        move || Ok(json!({"slots":slots}))
    }
    #[test]
    fn only_one_responding_supported_mouse_is_selected() {
        let slots = json!([{"slot":1,"supported_model":false},{"slot":3,"supported_model":true}]);
        let mut asked = vec![];
        let located = select(pairing(slots), |slot| {
            asked.push(slot);
            Ok("AABBCCDD".into())
        })
        .unwrap();
        assert_eq!(
            located,
            Located {
                slot: 3,
                identity: "AABBCCDD".into()
            }
        );
        assert_eq!(asked, vec![3]);
    }
    #[test]
    fn sleeping_absent_or_ambiguous_mice_are_errors_not_guesses() {
        let asleep = select(pairing(json!([{"slot":2,"supported_model":true}])), |_| {
            Err("timeout".into())
        })
        .unwrap_err();
        assert!(
            asleep.contains("activo") && asleep.contains("slot 2"),
            "{asleep}"
        );
        assert!(select(pairing(json!([])), |_| unreachable!())
            .unwrap_err()
            .contains("emparejado"));
        let two = json!([{"slot":1,"supported_model":true},{"slot":2,"supported_model":true}]);
        assert!(select(pairing(two), |s| Ok(format!("0000000{s}")))
            .unwrap_err()
            .contains("varios"));
        // One asleep and one online: the online mouse is unambiguous.
        let mixed = json!([{"slot":1,"supported_model":true},{"slot":2,"supported_model":true}]);
        let located = select(pairing(mixed), |s| {
            if s == 1 {
                Err("asleep".into())
            } else {
                Ok("11223344".into())
            }
        })
        .unwrap();
        assert_eq!(located.slot, 2);
        assert!(
            select(pairing(json!([{"slot":9,"supported_model":true}])), |_| {
                unreachable!()
            })
            .is_err()
        );
        assert!(select(|| Err("receiver busy".into()), |_| unreachable!()).is_err());
    }
}
