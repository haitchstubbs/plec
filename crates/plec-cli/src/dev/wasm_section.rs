/// Reader for WASM custom sections.
///
/// The runtime embeds its implemented protocol versions in a `plec-protocol`
/// custom section (see `crates/plec-runtime/src/lib.rs`), so a built or staged
/// binary can report which protocol it actually implements — the built
/// artifact may predate the current source constants. Only section walking is
/// implemented here; the binary format is a stream of
/// `id: u8, size: leb128, payload` records with custom sections as id 0.
pub fn read_custom_section(wasm: &[u8], name: &str) -> Result<Option<Vec<u8>>, String> {
    if wasm.len() < 8 || &wasm[0..4] != b"\0asm" {
        return Err("not a WASM binary (bad magic)".into());
    }

    let mut pos = 8usize;

    while pos < wasm.len() {
        let id = wasm[pos];
        pos += 1;

        let (size, next) = leb_u32(wasm, pos)?;
        pos = next;

        let payload_end = pos
            .checked_add(size)
            .ok_or("section size overflows the binary")?;
        if payload_end > wasm.len() {
            return Err("truncated section payload".into());
        }

        if id == 0 {
            let (name_len, name_start) = leb_u32(wasm, pos)?;
            let name_end = name_start
                .checked_add(name_len)
                .ok_or("custom section name overflows the payload")?;
            if name_end > payload_end {
                return Err("truncated custom section name".into());
            }

            if &wasm[name_start..name_end] == name.as_bytes() {
                return Ok(Some(wasm[name_end..payload_end].to_vec()));
            }
        }

        pos = payload_end;
    }

    Ok(None)
}

fn leb_u32(bytes: &[u8], start: usize) -> Result<(usize, usize), String> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    let mut pos = start;

    loop {
        let byte = *bytes.get(pos).ok_or("truncated LEB128 value")?;
        pos += 1;

        result |= u64::from(byte & 0x7f) << shift;

        if byte & 0x80 == 0 {
            break;
        }

        shift += 7;
        if shift > 35 {
            return Err("LEB128 value is too large for u32".into());
        }
    }

    let value = usize::try_from(result).map_err(|_| "LEB128 value exceeds address space")?;
    Ok((value, pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![id];
        let mut size = payload.len();
        loop {
            let mut byte = (size & 0x7f) as u8;
            size >>= 7;
            if size != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if size == 0 {
                break;
            }
        }
        out.extend_from_slice(payload);
        out
    }

    fn custom_section(name: &str, data: &[u8]) -> Vec<u8> {
        let mut payload = vec![(name.len() as u8)];
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(data);
        section(0, &payload)
    }

    fn wasm(sections: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"\0asm\x01\0\0\0".to_vec();
        for section in sections {
            out.extend_from_slice(section);
        }
        out
    }

    #[test]
    fn finds_the_named_custom_section() {
        let binary = wasm(&[
            section(1, &[0x60, 0, 0]),
            custom_section("plec-protocol", br#"{"ssrSnapshot":2}"#),
            custom_section("producers", b"other"),
        ]);

        let found = read_custom_section(&binary, "plec-protocol").unwrap();
        assert_eq!(found.as_deref(), Some(&br#"{"ssrSnapshot":2}"#[..]));
    }

    #[test]
    fn returns_none_for_missing_sections() {
        let binary = wasm(&[custom_section("producers", b"x")]);
        assert_eq!(read_custom_section(&binary, "plec-protocol").unwrap(), None);
    }

    #[test]
    fn rejects_non_wasm_input() {
        assert!(read_custom_section(b"not wasm", "x").is_err());
    }
}
