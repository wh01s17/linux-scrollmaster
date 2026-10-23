// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline JSON-in/JSON-out profile tool. No filesystem writes or hardware.
use linux_scrollmaster::profiles::{Document, MAX_DOCUMENT_BYTES};
use serde_json::{json, Value};
use std::io::{self, Read};

const MAX_FRAME: usize = 2 * MAX_DOCUMENT_BYTES + 4096;
fn read_input(mut reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((MAX_FRAME + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FRAME {
        return Err("Profile input frame is too large".into());
    }
    Ok(bytes)
}
fn validate_args(args: &[String]) -> Result<(), String> {
    if args.len() != 1 || !["--validate", "--manage"].contains(&args[0].as_str()) {
        return Err("Uso: native_profiles --validate < documento.json; o --manage con {document,request} por stdin. Solo calcula JSON; no guarda ni aplica perfiles.".into());
    }
    Ok(())
}
fn run(args: &[String], bytes: &[u8]) -> Result<Value, String> {
    validate_args(args)?;
    if bytes.len() > MAX_FRAME {
        return Err("Profile input frame is too large".into());
    }
    let data = match args[0].as_str() {
        "--validate" => {
            let document = Document::parse(bytes)?;
            json!({"version":document.version(),"profiles":document.profiles().len()})
        }
        "--manage" => {
            let frame: Value =
                serde_json::from_slice(bytes).map_err(|_| "Invalid profile input JSON")?;
            let fields = frame.as_object().ok_or("Expected document/request frame")?;
            if fields.len() != 2
                || !fields.contains_key("document")
                || !fields.contains_key("request")
            {
                return Err("Expected exactly document and request".into());
            }
            let document = Document::from_value(frame["document"].clone())?;
            let result = document.manage(&frame["request"])?;
            json!({"document":result.document.as_value(),"transfer":result.transfer,"changed":result.changed})
        }
        _ => unreachable!(),
    };
    Ok(json!({"stage":"N5","experimental":true,"read_only":true,"data":data}))
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let response = validate_args(&args)
        .and_then(|_| read_input(io::stdin().lock()))
        .and_then(|bytes| run(&args, &bytes));
    match response {
        Ok(data) => println!("{}", json!({"ok":true,"data":data})),
        Err(error) => {
            println!("{}", json!({"ok":false,"error":error}));
            std::process::exit(1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(value: &str) -> Vec<String> {
        vec![value.into()]
    }
    #[test]
    fn validation_requires_a_document_and_has_no_activation_output() {
        let result = run(
            &args("--validate"),
            br#"{"version":1,"profiles":[],"active":{}}"#,
        )
        .unwrap();
        assert_eq!(result["read_only"], true);
        assert_eq!(result["data"], json!({"version":1,"profiles":0}));
        assert!(run(&args("--validate"), b"{}").is_err());
    }
    #[test]
    fn management_only_returns_a_plan_and_retains_unknown_metadata() {
        let frame = json!({"document":{"version":1,"profiles":[{"device":"a","name":"Old","settings":{"dpi":800}}],"active":{"a":"Old"},"note":"keep"},"request":{"op":"rename","device":"a","name":"Old","new_name":"New"}});
        let result = run(&args("--manage"), &serde_json::to_vec(&frame).unwrap()).unwrap();
        assert_eq!(result["read_only"], true);
        assert_eq!(result["data"]["changed"], true);
        assert_eq!(result["data"]["document"]["active"]["a"], "New");
        assert_eq!(result["data"]["document"]["note"], "keep");
    }
    #[test]
    fn paths_extra_fields_and_hardware_operations_are_rejected() {
        for args in [
            vec![],
            args("--save"),
            vec!["--validate".into(), "profiles.json".into()],
        ] {
            assert!(run(&args, b"{}").is_err());
        }
        for frame in [
            json!({"document":{},"request":{},"path":"/tmp/forbidden"}),
            json!({"document":{"version":1,"profiles":[],"active":{}},"request":{"op":"restore","device":"a","name":"x"}}),
        ] {
            assert!(run(&args("--manage"), &serde_json::to_vec(&frame).unwrap()).is_err());
        }
    }
    #[test]
    fn input_and_individual_document_size_limits_are_independent() {
        assert!(read_input(&vec![b'x'; MAX_FRAME + 1][..]).is_err());
        assert!(run(&args("--validate"), &vec![b' '; MAX_DOCUMENT_BYTES + 1]).is_err());
        assert!(run(&args("--manage"), &vec![b' '; MAX_FRAME + 1]).is_err());
        assert!(read_input(&b"{}"[..]).is_ok());
    }
    #[test]
    fn input_errors_and_malformed_json_propagate_without_a_plan() {
        struct Failed;
        impl Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("synthetic read failure"))
            }
        }
        assert!(read_input(Failed)
            .unwrap_err()
            .contains("synthetic read failure"));
        assert!(run(&args("--manage"), b"{broken").is_err());
    }
}
