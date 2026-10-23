// SPDX-License-Identifier: GPL-3.0-or-later
//! Small fail-closed HID item parser for the dedicated Bolt HID++ interface.
#[derive(Clone, Copy, Default)]
struct Globals {
    page: u32,
    size: u32,
    count: u32,
    id: u32,
}

pub(super) fn validate(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err("Invalid descriptor length".into());
    }
    let mut g = Globals::default();
    let mut stack = Vec::new();
    let mut depth = 0usize;
    let mut bits = [[0u32; 2]; 2];
    let mut offset = 0;
    while offset < bytes.len() {
        let prefix = bytes[offset];
        offset += 1;
        if prefix == 0xfe {
            return Err("Long HID items are unsupported".into());
        }
        let size = match prefix & 3 {
            3 => 4,
            n => n as usize,
        };
        let data = bytes
            .get(offset..offset + size)
            .ok_or("Truncated HID item")?;
        offset += size;
        let value = data
            .iter()
            .enumerate()
            .fold(0u32, |v, (i, b)| v | ((*b as u32) << (8 * i)));
        match ((prefix >> 2) & 3, prefix >> 4) {
            (1, 0) => g.page = value,
            (1, 7) => g.size = value,
            (1, 8) if (1..=255).contains(&value) => g.id = value,
            (1, 8) => return Err("Invalid report ID".into()),
            (1, 9) => g.count = value,
            (1, 10) if stack.len() < 8 => stack.push(g),
            (1, 10) => return Err("HID global stack overflow".into()),
            (1, 11) => g = stack.pop().ok_or("HID global stack underflow")?,
            (0, 10) if depth < 8 => depth += 1,
            (0, 10) => return Err("HID collection depth exceeded".into()),
            (0, 12) if depth > 0 => depth -= 1,
            (0, 12) => return Err("HID collection underflow".into()),
            (0, tag @ (8 | 9)) => {
                if depth == 0
                    || g.page != 0xff00
                    || !(1..=32).contains(&g.size)
                    || g.count == 0
                    || g.count > 4096
                {
                    return Err("Not a dedicated vendor HID++ interface".into());
                }
                let report = match g.id {
                    0x10 => 0,
                    0x11 => 1,
                    _ => return Err("Unexpected input/output report".into()),
                };
                let direction = usize::from(tag == 9);
                bits[report][direction] = bits[report][direction]
                    .checked_add(g.size * g.count)
                    .ok_or("HID report overflow")?;
            }
            (0, 11) => return Err("Unexpected feature report".into()),
            (3, _) => return Err("Reserved HID item".into()),
            _ => {}
        }
    }
    if depth != 0 || !stack.is_empty() || bits != [[48, 48], [152, 152]] {
        return Err("Expected bidirectional HID++ reports of 7/20 bytes".into());
    }
    Ok(())
}
