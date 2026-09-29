//! Quantum Sponge Hash (QSH) — EXACT port of the engine's
//! `security/quantum_signer.go` (KSP-6B hardened sponge):
//! multi-rate padding, 24 rounds with round constants, modular-addition
//! non-linearity, and domain separation.
//!
//! Unit tests pin engine vectors (see `vectors` below), so any drift
//! between this CLI and the chain fails the build.

/// Round constants (decorative fixed entropy, matches engine).
const ROUND_CONSTANTS: [u8; 24] = [
    0x6a, 0x09, 0xe6, 0x67, 0xf3, 0xbc, 0xc9, 0x0b, 0xbb, 0x67, 0xae, 0x85, 0x84, 0xca, 0xa7, 0x3b,
    0x3c, 0x6e, 0xf3, 0x72, 0xa5, 0x4f, 0xf5, 0x3a,
];

/// Domain separation tags (must match engine `Domain*` constants).
pub const DOMAIN_KSEL8: &str = "KSEL-8";
pub const DOMAIN_ADDRESS: &str = "KDC-ADDR";
pub const DOMAIN_CREATE: &str = "KDC-CREATE";
pub const DOMAIN_KVI_ID: &str = "KVI-ID";
pub const DOMAIN_KVM_QSH: &str = "KVM-QSH";

struct Sponge {
    state: [u8; 32],
    pos: usize,
}

impl Sponge {
    fn new() -> Self {
        Self {
            state: [0u8; 32],
            pos: 0,
        }
    }

    fn absorb(&mut self, data: &[u8]) {
        for &b in data {
            self.state[self.pos] ^= b;
            self.pos += 1;
            if self.pos == 32 {
                self.permute();
                self.pos = 0;
            }
        }
    }

    /// Multi-rate padding + final permutation (call once after absorb).
    fn finish(&mut self) {
        if self.pos == 32 {
            self.permute();
            self.pos = 0;
        }
        self.state[self.pos] ^= 0x01;
        self.state[31] ^= 0x80;
        self.permute();
        self.pos = 0;
    }

    /// Domain barrier: seal the previously absorbed segment.
    fn barrier(&mut self) {
        if self.pos != 0 {
            self.permute();
            self.pos = 0;
        }
    }

    fn permute(&mut self) {
        for r in 0..24 {
            for i in 0..32 {
                let next = self.state[(i + 1) % 32];
                self.state[i] ^= next ^ self.state[(i + 7) % 32] ^ ROUND_CONSTANTS[(r + i) % 24];
                let rot = ((r + i) % 8) + 1;
                self.state[i] = self.state[i].rotate_left(rot as u32);
                self.state[i] = self.state[i].wrapping_add(next);
            }
        }
    }

    fn squeeze(&mut self, out: &mut [u8]) {
        for slot in out.iter_mut() {
            if self.pos == 32 {
                self.permute();
                self.pos = 0;
            }
            *slot = self.state[self.pos];
            self.pos += 1;
        }
    }
}

/// Generic hardened QSH (32 bytes).
pub fn qsh(data: &[u8]) -> [u8; 32] {
    let mut s = Sponge::new();
    s.absorb(data);
    s.finish();
    let mut out = [0u8; 32];
    s.squeeze(&mut out);
    out
}

/// Domain-separated QSH (32 bytes).
pub fn qsh_tagged(domain: &str, data: &[u8]) -> [u8; 32] {
    let mut s = Sponge::new();
    s.absorb(domain.as_bytes());
    s.barrier();
    s.absorb(data);
    s.finish();
    let mut out = [0u8; 32];
    s.squeeze(&mut out);
    out
}

/// 0x-hex of the generic hash.
pub fn qsh_hex(data: &[u8]) -> String {
    format!("0x{}", hex::encode(qsh(data)))
}

/// 0x-hex of a tagged hash.
pub fn qsh_tagged_hex(domain: &str, data: &[u8]) -> String {
    format!("0x{}", hex::encode(qsh_tagged(domain, data)))
}

/// KSEL-8 selector: first 4 bytes of the QSH in the KSEL-8 domain.
pub fn ksel8(signature: &str) -> String {
    hex::encode(&qsh_tagged(DOMAIN_KSEL8, signature.as_bytes())[..4])
}

/// Canonical wallet address: QSH(pubkey) in KDC-ADDR → last 40 hex + 0x.
pub fn address_hex(pubkey: &[u8]) -> String {
    let full = hex::encode(qsh_tagged(DOMAIN_ADDRESS, pubkey));
    format!("0x{}", &full[full.len() - 40..])
}

/// KVI identity of an address (what CALLER/ADDRESS push): full QSH in KVI-ID.
pub fn identity_hex(address: &str) -> String {
    qsh_tagged_hex(DOMAIN_KVI_ID, address.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_vectors_match_engine() {
        // Pinned against the live engine (KSP-6B avalanche run).
        assert_eq!(
            qsh_hex(b"abc"),
            "0x7e8da7214b735039feea2549dee66ef52bfca8ebb4cd3a294760f542b2b96d19"
        );
        assert_eq!(
            qsh_hex(b""),
            "0xe8236088af43c4d7d228161bf2fe40698ab004cc1f495fa3c7c6f695b7682469"
        );
    }

    #[test]
    fn tagged_vectors_match_engine() {
        // Pinned against Go security.GenerateQuantumHashTagged* + the TS
        // SDK (all three agree byte-for-byte; verified 2026-09-28).
        assert_eq!(
            qsh_tagged_hex(DOMAIN_KSEL8, b"init(address)"),
            "0xee44147b88dc672b5773c07767a00b19056f6dd304c0d13a3d2fe27d9c1c2d2b"
        );
        assert_eq!(
            qsh_tagged_hex(DOMAIN_KVI_ID, b"0x66d1acb3f9065eabecd97b0c2df07de45245518f"),
            "0x5902667e00618422efafe3d9cdd9a99080edc96c3cec242b0a029560f9412025"
        );
    }

    #[test]
    fn ksel8_matches_frozen_table() {
        // KSP-1/2/3 frozen selectors (post-KSP-6B regeneration).
        assert_eq!(ksel8("transfer(address,u256)"), "5a7f292d");
        assert_eq!(ksel8("init(address)"), "ee44147b");
        assert_eq!(ksel8("balanceOf(address)"), "1f39dfb2");
    }

    #[test]
    fn avalanche_sanity() {
        // One input bit flips ~half the output bits (50.2% measured live).
        let a = qsh(b"kode0");
        let mut b_in = *b"kode0";
        b_in[4] ^= 1;
        let b = qsh(&b_in);
        let diff: u32 = a
            .iter()
            .zip(b.iter())
            .map(|(x, y)| (x ^ y).count_ones())
            .sum();
        assert!((100..=160).contains(&diff), "diff bits = {diff}");
    }

    #[test]
    fn previously_colliding_selectors_differ() {
        // The weak-sponge collision class (KSP-3): must differ from byte 0.
        assert_ne!(ksel8("getReserveA()"), ksel8("getReserveB()"));
        assert_ne!(ksel8("getTokenA()"), ksel8("getTokenB()"));
    }
}
