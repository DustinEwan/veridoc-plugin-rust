//! The identity hashes the plugin computes.
//!
//! A symbol's `referentId` is `"vd:r:" + hex12(sha256(rootRelativePosixPath + "\u{0}" + name))`,
//! the construction the TypeScript plugin uses (`packages/plugin-ts/src/ids.ts:10-15`), so the
//! same-named item in two files gets two identities. The signature is excluded on purpose: a
//! signature edit must not move a claim's identity and with it its verdict cache. A concept has
//! A claim's id is the engine's own hash, `c + hex16(sha256(referent_id + "\u{0}" + predicate))`
//! (`crates/veridoc-engine/src/hash.rs:12-22`). `run` addresses a leaf as `<claim id>#l<n>`, so a
//! plugin that cannot recompute the id cannot find the claim the leaf belongs to.

use sha2::{Digest, Sha256};

/// The identity of one symbol in one file.
pub fn referent_id(root_relative_path: &str, name: &str) -> String {
    hashed("vd:r:", root_relative_path, name)
}

/// The identity of one declared concept.
pub fn concept_id(name: &str) -> String {
    hashed("vd:c:", "", name)
}

/// A claim's identity, recomputed exactly as the engine computes it.
pub fn claim_id(referent: &str, predicate: &str) -> String {
    format!("c{}", &joined(referent, predicate)[..16])
}

fn hashed(prefix: &str, discriminator: &str, name: &str) -> String {
    format!("{prefix}{}", &joined(discriminator, name)[..12])
}

/// `hex(sha256(a + "\u{0}" + b))`, the digest every identity in this file is cut from. The hex
/// is ASCII, so the callers above can slice it by byte index.
fn joined(a: &str, b: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(a.as_bytes());
    hasher.update([0u8]);
    hasher.update(b.as_bytes());
    hex(&hasher.finalize())
}

fn hex(digest: &[u8]) -> String {
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::{concept_id, referent_id};

    #[test]
    fn an_identity_is_the_prefix_and_twelve_hex_digits() {
        let id = referent_id("src/cli.ts", "probe");
        assert!(id.starts_with("vd:r:"), "{id}");
        assert_eq!(id.len(), "vd:r:".len() + 12, "{id}");
        assert!(id["vd:r:".len()..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_same_symbol_is_stable_and_a_different_file_is_not() {
        assert_eq!(
            referent_id("src/config.rs", "parse_config"),
            referent_id("src/config.rs", "parse_config")
        );
        assert_ne!(
            referent_id("src/config.rs", "parse_config"),
            referent_id("src/other.rs", "parse_config")
        );
    }

    #[test]
    fn a_concept_cannot_collide_with_a_symbol() {
        let concept = concept_id("configuration");
        assert!(concept.starts_with("vd:c:"), "{concept}");
        assert_ne!(concept, referent_id("configuration", "configuration"));
    }
}
