//! The identity hashes the plugin computes.
//!
//! A symbol's `referentId` is `"vd:r:" + hex12(sha256(rootRelativePosixPath + "\u{0}" + name))`,
//! the construction the TypeScript plugin uses (`packages/plugin-ts/src/ids.ts:10-15`), so the
//! same-named item in two files gets two identities. The signature is excluded on purpose: a
//! signature edit must not move a claim's identity and with it its verdict cache. A concept has
//! no file, so it carries `vd:c:` instead of a discriminator that could collide with a path.

use sha2::{Digest, Sha256};

/// The identity of one symbol in one file.
pub fn referent_id(root_relative_path: &str, name: &str) -> String {
    hashed("vd:r:", root_relative_path, name)
}

/// The identity of one declared concept.
pub fn concept_id(name: &str) -> String {
    hashed("vd:c:", "", name)
}

fn hashed(prefix: &str, discriminator: &str, name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(discriminator.as_bytes());
    hasher.update([0u8]);
    hasher.update(name.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("{prefix}{}", &hex[..12])
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
