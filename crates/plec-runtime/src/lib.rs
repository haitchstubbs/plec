pub mod lifecycle;
pub mod snapshots;
pub use lifecycle::PlecRuntime;

/// Protocol versions implemented by this binary, emitted as a WASM custom
/// section (`plec-protocol`) so built and staged artifacts can report which
/// serialized protocol they actually implement — a stale binary compiled
/// against snapshot v1 is detectable after the source bumps to v2.
///
/// `plec workspace artifact provenance` / `plec workspace artifact stale`
/// read this section out of
/// `packages/plec-runtime/dist/runtime/runtime_bg.wasm` and
/// its staged copy under `apps/fullstack/dist/runtime`. The value derives
/// from the plec-ir constant at compile time, so it cannot drift from the
/// snapshot schema itself.
#[used]
#[link_section = "plec-protocol"]
static PLEC_PROTOCOL: [u8; PLEC_PROTOCOL_LEN] = protocol_marker_bytes();

const fn protocol_marker_bytes() -> [u8; PLEC_PROTOCOL_LEN] {
    const PREFIX: &[u8] = b"{\"ssrSnapshot\":";

    let mut out = [0u8; PLEC_PROTOCOL_LEN];

    let mut position = 0;
    let mut index = 0;
    while index < PREFIX.len() {
        out[position] = PREFIX[index];
        position += 1;
        index += 1;
    }

    // const fn itoa: format `SSR_SNAPSHOT_VERSION` in decimal.
    let mut digits = [0u8; 10];
    let mut count = 0;
    let mut value = plec_ir::SSR_SNAPSHOT_VERSION;

    if value == 0 {
        digits[0] = b'0';
        count = 1;
    }
    while value > 0 {
        digits[count] = b'0' + (value % 10) as u8;
        count += 1;
        value /= 10;
    }

    let mut remaining = count;
    while remaining > 0 {
        remaining -= 1;
        out[position] = digits[remaining];
        position += 1;
    }

    out[position] = b'}';
    out
}

const PLEC_PROTOCOL_LEN: usize = {
    const PREFIX_LEN: usize = "{\"ssrSnapshot\":".len();
    const SUFFIX_LEN: usize = 1;

    PREFIX_LEN + decimal_digits(plec_ir::SSR_SNAPSHOT_VERSION) + SUFFIX_LEN
};

const fn decimal_digits(mut value: u32) -> usize {
    let mut count = 1;
    while value >= 10 {
        value /= 10;
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    /// The embedded marker must describe the snapshot protocol this crate
    /// actually gates on, never a hand-maintained copy of it.
    #[test]
    fn protocol_marker_agrees_with_snapshot_constant() {
        let marker = core::str::from_utf8(&super::PLEC_PROTOCOL).expect("marker is ASCII");
        assert_eq!(
            marker,
            format!("{{\"ssrSnapshot\":{}}}", plec_ir::SSR_SNAPSHOT_VERSION)
        );
    }
}
