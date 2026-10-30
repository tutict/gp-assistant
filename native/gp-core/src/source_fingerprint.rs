use sha2::{Digest, Sha256};

/// Implementation identity protocol: CRLF pairs become LF before both length
/// framing and hashing. All other bytes (including lone CR) remain significant.
/// Use only for source text, never dataset bytes, reports or parameter digests.
pub fn source_text_fingerprint(domain: &[u8], sources: &[(&str, &[u8])]) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    for (name, source) in sources {
        let normalized = source
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(index, byte)| {
                (byte != b'\r' || source.get(index + 1) != Some(&b'\n')).then_some(byte)
            })
            .collect::<Vec<_>>();
        hash.update(name.as_bytes());
        hash.update((normalized.len() as u64).to_le_bytes());
        hash.update(normalized);
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_source_text_identity_normalizes_lengths_and_newlines() {
        let fingerprint =
            |bytes: &[u8]| source_text_fingerprint(b"test/v1\0", &[("source.rs", bytes)]);
        let lf = fingerprint(b"first\nsecond\n");
        assert_eq!(lf, fingerprint(b"first\r\nsecond\r\n"));
        assert_eq!(lf, fingerprint(b"first\r\nsecond\n"));
        assert_ne!(lf, fingerprint(b"first\nchanged\n"));
        assert_ne!(lf, fingerprint(b"first\nsecond\r"));
        assert_ne!(
            lf,
            source_text_fingerprint(b"test/v1\0", &[("other.rs", b"first\nsecond\n")])
        );
    }
}
