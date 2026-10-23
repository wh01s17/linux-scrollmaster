// SPDX-License-Identifier: GPL-3.0-or-later
//! Fixed read-only query catalog. No public feature/function dispatcher.
use crate::hidpp::{ReportKind, Session, Transport};
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const IDS: &[u16] = &[
    0x0003, 0x0005, 0x1000, 0x1004, 0x1b04, 0x2110, 0x2111, 0x2121, 0x2150, 0x2201,
];
#[derive(Clone, Copy)]
struct Feature {
    id: u16,
    index: u8,
    flags: u8,
    version: u8,
}
struct Reader<'a, T> {
    session: &'a mut Session<T>,
    slot: u8,
    deadline: Instant,
}
impl<T: Transport> Reader<'_, T> {
    fn read(&mut self, index: u8, function: u8, parameters: &[u8]) -> Result<Vec<u8>, String> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or("Diagnostic deadline elapsed")?;
        let report = self
            .session
            .request(
                if index == 0 {
                    ReportKind::Short
                } else {
                    ReportKind::Long
                },
                self.slot,
                index,
                function,
                parameters,
                remaining.min(Duration::from_secs(2)),
            )
            .map_err(|e| e.to_string())?;
        // Preserve interleaved reports for the session's event consumer. The
        // bounded session queue fails closed on overflow; never drain silently.
        Ok(report.payload().to_vec())
    }
    fn lookup(&mut self, id: u16) -> Result<Option<Feature>, String> {
        let p = self.read(0, 0, &id.to_be_bytes())?;
        if p.len() < 3 {
            return Err("Truncated feature lookup".into());
        }
        if p[0] == 0 {
            return Ok(None);
        }
        if matches!(p[0], 0x8f | 0xff) {
            return Err("Reserved feature index".into());
        }
        Ok(Some(Feature {
            id,
            index: p[0],
            flags: p[1],
            version: p[2],
        }))
    }
    fn get(
        &mut self,
        features: &BTreeMap<u16, Feature>,
        id: u16,
        function: u8,
        params: &[u8],
        needed: usize,
    ) -> Result<Vec<u8>, String> {
        let f = features
            .get(&id)
            .ok_or_else(|| format!("Feature {id:04X} not advertised"))?;
        if f.flags & 0xc0 != 0 {
            return Err("Hidden/obsolete feature excluded from diagnostic".into());
        }
        // These are the layouts covered by N2 fixtures; future revisions need review.
        if f.version > 4 {
            return Err(format!(
                "Feature {id:04X} version {} not validated",
                f.version
            ));
        }
        let p = self.read(f.index, function, params)?;
        if p.len() < needed {
            return Err(format!("Truncated {id:04X} response"));
        }
        Ok(p)
    }
    fn name(&mut self, features: &BTreeMap<u16, Feature>) -> Result<String, String> {
        let size = self.get(features, 0x0005, 0, &[], 1)?[0] as usize;
        if size == 0 || size > 80 {
            return Err("Invalid device name length".into());
        }
        let mut bytes = Vec::new();
        while bytes.len() < size {
            let p = self.get(features, 0x0005, 1, &[bytes.len() as u8], 1)?;
            bytes.extend_from_slice(&p[..p.len().min(size - bytes.len())]);
        }
        let name = String::from_utf8(bytes).map_err(|_| "Invalid UTF-8 device name")?;
        if name.chars().any(char::is_control) {
            return Err("Control character in device name".into());
        }
        Ok(name)
    }
}

fn unavailable(error: impl ToString) -> Value {
    json!({"available":false,"value":null,"error":error.to_string()})
}
fn reading(value: Value, raw: &[u8]) -> Value {
    json!({"available":true,"value":value,"raw":hex(raw)})
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Takes ownership of a fresh, exclusively owned transport. The caller must check
/// conflicts and validate the interface. Only receiver slots 1..6 are accepted.
pub fn diagnose<T: Transport>(transport: T, slot: u8, deadline: Instant) -> Result<Value, String> {
    diagnose_session(&mut Session::new(transport), slot, deadline)
}

pub(super) fn diagnose_session<T: Transport>(
    session: &mut Session<T>,
    slot: u8,
    deadline: Instant,
) -> Result<Value, String> {
    if !(1..=6).contains(&slot) {
        return Err("Choose an explicit Bolt slot from 1 to 6".into());
    }
    let mut r = Reader {
        session,
        slot,
        deadline,
    };
    // Fresh token and echo prevent accepting an old root response from an earlier session.
    let token = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .subsec_nanos() as u8;
    let ping = r.read(0, 1, &[0, 0, token])?;
    if ping.len() < 3 || ping[2] != token || ping[0] < 2 {
        return Err("Protocol ping/echo did not validate HID++ 2.0+".into());
    }
    let mut features = BTreeMap::new();
    let mut indices = BTreeMap::new();
    let mut catalog = Vec::new();
    for &id in IDS {
        if let Some(f) = r.lookup(id)? {
            if indices.insert(f.index, id).is_some() {
                return Err("Duplicate feature index in discovery".into());
            }
            catalog.push(json!({"id":format!("{:04X}",f.id),"index":f.index,"flags":f.flags,"version":f.version}));
            features.insert(id, f);
        }
    }
    let info = r.get(&features, 0x0003, 0, &[], 13)?;
    let unit = &info[1..5];
    if unit == [0; 4] || unit == [0xff; 4] {
        return Err("Missing stable Unit ID; no slot-based identity is invented".into());
    }
    let mut model_offset = 7;
    let mut bolt_model = None;
    for mask in [1, 2, 4, 8] {
        if info[6] & mask != 0 {
            let model = info
                .get(model_offset..model_offset + 2)
                .ok_or("Truncated model IDs")?;
            // Bolt transports BLE devices; their model is the BLE ID, not eQuad.
            if mask == 2 {
                bolt_model = Some(hex(model));
            }
            model_offset += 2;
        }
    }
    if bolt_model.as_deref() != Some("B034") {
        return Err(format!(
            "MX Master 3S model validation failed: transport flags {:02X}, model IDs {}",
            info[6],
            hex(&info[7..13])
        ));
    }
    let name = r.name(&features)?;
    if name != "MX Master 3S" {
        return Err(format!("Unsupported device name: {name}"));
    }
    let mut settings = Map::new();
    for key in [
        "dpi",
        "smart-shift",
        "scroll-ratchet",
        "hires-smooth-invert",
        "hires-smooth-resolution",
        "thumb-scroll-invert",
    ] {
        settings.insert(key.into(), unavailable("Feature absent or read pending"));
    }
    let dpi = (|| -> Result<Value, String> {
        let sensors = r.get(&features, 0x2201, 0, &[], 1)?;
        if sensors[0] != 1 {
            return Err("N2 supports exactly one DPI sensor".into());
        }
        let list = r.get(&features, 0x2201, 1, &[0, 0, 0], 3)?;
        if list[0] != 0 {
            return Err("DPI list sensor echo mismatch".into());
        }
        let choices = parse_dpi_list(&list[1..])?;
        let p = r.get(&features, 0x2201, 2, &[0], 5)?;
        if p[0] != 0 {
            return Err("DPI value sensor echo mismatch".into());
        }
        let current = u16::from_be_bytes([p[1], p[2]]);
        let value = if current == 0 {
            u16::from_be_bytes([p[3], p[4]])
        } else {
            current
        };
        if !choices.contains(&value) {
            return Err("Current DPI is outside advertised choices".into());
        }
        let mut result = reading(json!(value), &p);
        result["choices"] = json!(choices);
        result["capabilities_raw"] = json!(hex(&list));
        Ok(result)
    })();
    settings.insert("dpi".into(), dpi.unwrap_or_else(unavailable));

    let shift_id = if features.contains_key(&0x2111) {
        0x2111
    } else {
        0x2110
    };
    match r.get(
        &features,
        shift_id,
        if shift_id == 0x2111 { 1 } else { 0 },
        &[],
        2,
    ) {
        Ok(p) if matches!(p[0], 1 | 2) => {
            settings.insert("scroll-ratchet".into(), reading(json!(p[0]), &p));
            let mut value = reading(json!(if p[0] == 1 { 1 } else { p[1].min(50) }), &p);
            value["threshold_hidden_in_freespin"] = json!(p[0] == 1);
            if p[0] == 2 && p[1] == 0 {
                value = unavailable("Invalid zero SmartShift threshold");
            }
            settings.insert("smart-shift".into(), value);
        }
        result => {
            let error = result.err().unwrap_or("Invalid wheel mode".into());
            for key in ["smart-shift", "scroll-ratchet"] {
                settings.insert(key.into(), unavailable(&error));
            }
        }
    }
    let mut capabilities = json!({});
    let hires = (|| -> Result<(Vec<u8>, Vec<u8>), String> {
        let caps = r.get(&features, 0x2121, 0, &[], 2)?;
        let mode = r.get(&features, 0x2121, 1, &[], 1)?;
        Ok((caps, mode))
    })();
    match hires {
        Ok((caps, p)) => {
            capabilities["hires-wheel"] = json!({"raw":hex(&caps),"diverted":p[0]&1!=0});
            settings.insert(
                "hires-smooth-invert".into(),
                if caps[1] & 8 != 0 {
                    reading(json!(p[0] & 4 != 0), &p)
                } else {
                    unavailable("Inversion capability not advertised")
                },
            );
            settings.insert(
                "hires-smooth-resolution".into(),
                reading(json!(p[0] & 2 != 0), &p),
            );
        }
        Err(error) => {
            for key in ["hires-smooth-invert", "hires-smooth-resolution"] {
                settings.insert(key.into(), unavailable(&error));
            }
        }
    }
    match r.get(&features, 0x2150, 1, &[], 2) {
        Ok(p) => {
            capabilities["thumb-wheel"] = json!({"diverted":p[0]&1!=0});
            settings.insert(
                "thumb-scroll-invert".into(),
                reading(json!(p[1] & 1 != 0), &p),
            );
        }
        Err(error) => {
            settings.insert("thumb-scroll-invert".into(), unavailable(error));
        }
    }
    let battery=(|| -> Result<Value,String> {
        let (id,function)=if features.contains_key(&0x1004){(0x1004,1)}else{(0x1000,0)};
        let p=r.get(&features,id,function,&[],if id==0x1004{4}else{3})?;
        if p[0]>100 {return Err("Invalid battery percentage".into());}
        // 0 can mean unknown. Keep the level/status raw instead of inventing a percentage.
        Ok(json!({"available":true,"percentage":if p[0]>0{Some(p[0])}else{None},"level_code":p[1],"status":p[2],"feature":format!("{id:04X}"),"raw":hex(&p)}))
    })().unwrap_or_else(unavailable);
    if r.session.is_closed() {
        return Err(
            "The transport failed during diagnostic; partial readings are not a valid snapshot"
                .into(),
        );
    }
    Ok(
        json!({"backend":"native-hidpp","stage":"N2","read_only":true,"experimental":true,
        "device":{"id":hex(unit),"name":name,"connection":"Logi Bolt","slot":slot,"model":bolt_model,"model_ids_raw":hex(&info[7..13]),
            "protocol":{"major":ping[0],"minor":ping[1]},"battery":battery,"settings":settings},
        "features":catalog,"feature_scope":"known-read-only-subset","capabilities":capabilities}),
    )
}

pub(super) fn parse_dpi_list(bytes: &[u8]) -> Result<Vec<u16>, String> {
    let mut values = Vec::new();
    let mut words = bytes.as_chunks::<2>().0.iter();
    while let Some(word) = words.next() {
        let value = u16::from_be_bytes(*word);
        if value == 0 {
            if values.is_empty() {
                return Err("Empty DPI list".into());
            }
            return Ok(values);
        }
        if value & 0xe000 == 0xe000 {
            let step = value & 0x1fff;
            let start = *values.last().ok_or("DPI range has no start")?;
            let end = u16::from_be_bytes(*words.next().ok_or("Truncated DPI range")?);
            if step == 0 || end <= start || end >= 0xe000 || (end - start) % step != 0 {
                return Err("Invalid DPI range".into());
            }
            let count = ((end - start) / step) as usize;
            if values.len() + count > 2048 {
                return Err("DPI choices limit exceeded".into());
            }
            for i in 1..=count {
                values.push(start + step * i as u16);
            }
        } else {
            if values.last().is_some_and(|last| value <= *last) || values.len() == 2048 {
                return Err("Invalid DPI ordering/size".into());
            }
            values.push(value);
        }
    }
    Err("Unterminated DPI list: multi-page DPI layouts are not supported in N2".into())
}
