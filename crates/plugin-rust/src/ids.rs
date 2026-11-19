//! The identities and the claim names the plugin computes, in one place so the rules cannot drift.
//!
//! A symbol's `referentId` is
//! `"veridoc:referent:" + hex12(sha256(rootRelativePosixPath + "\u{0}" + name))`, the construction
//! the TypeScript plugin uses (`packages/plugin-ts/src/ids.ts`), so the same-named item in two
//! files gets two identities. The signature is excluded on purpose: a signature edit must not move
//! a claim's identity and with it its verdict cache. A concept has the same construction under
//! `veridoc:concept:`. Both are reporting values now: `resolve` answers with them and the referent
//! table prints them, and no claim name is built from them any more.
//!
//! A claim's identity is `c + hex16(sha256(subject + "\u{0}" + predicate))` and the name the engine
//! binds for it is `<subject>#<slug>`, both computed by `claim_identity` and `claim_name` in
//! `crates/veridoc-engine/src/hash.rs`. The subject is the author's name for the thing the claim is
//! about -- `RefToken::subject_name` in `crates/veridoc-protocol/src/contract.rs`, the symbol's name
//! for a symbol subject and the concept's name for a concept subject -- and never a path or a
//! referent id, so renaming the file that holds the code moves no claim
//! (`docs/decisions/0009-the-subject-is-named-not-derived-from-a-path.md`). The slug is the
//! predicate lower-cased, split on every non-alphanumeric character, empty pieces dropped, words
//! joined by `-`, kept to the last whole word that fits in 48 characters (`hash.rs::slug`).
//! `bivalence` is asked by name and `run` addresses a leaf as `<claim name>#l<n>`, so a plugin that
//! cannot recompute the name cannot find the claim it was asked about. Two predicates of one project
//! can slug alike, and then every claim sharing that name carries the identity suffix, `~` plus the
//! six hex digits after the identity's leading `c` (the renaming pass in
//! `crates/veridoc-engine/src/bind.rs`) -- which is why the identity is computed here too.

use sha2::{Digest, Sha256};

/// The identity of one symbol in one file.
pub fn referent_id(root_relative_path: &str, name: &str) -> String {
    hashed("veridoc:referent:", root_relative_path, name)
}

/// The identity of one declared concept.
pub fn concept_id(name: &str) -> String {
    hashed("veridoc:concept:", "", name)
}

/// A claim's identity, recomputed exactly as the engine computes it from the author's subject name
/// and the predicate. It is not a name: the engine prints it only as the collision suffix, which is
/// the one thing a plugin still needs it for.
pub fn claim_identity(subject: &str, predicate: &str) -> String {
    format!("c{}", &joined(subject, predicate)[..16])
}

/// A claim's name, `<subject>#<slug>`, as the engine computes it. The rule itself lives in
/// `veridoc-claims`, the library the project's claim modules link, because the project's harness
/// resolves the name it is handed by that one implementation: a second copy here could report a
/// name the harness cannot find. Two claims of one project can slug alike, and bind then adds
/// `suffix(claim_identity(..))` to every one of them; that pass belongs to the whole claim set, so
/// it lives in `project::Project::claim_name`, not here.
pub fn claim_name(subject: &str, predicate: &str) -> String {
    veridoc_claims::claim_name(subject, predicate)
}

/// The identity shortened for use as a collision suffix, so two claims that slug alike stay
/// distinguishable without reprinting the whole digest.
pub fn suffix(identity: &str) -> String {
    format!("~{}", &identity[1..7])
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
    use super::{claim_identity, claim_name, concept_id, referent_id, suffix};

    #[test]
    fn an_identity_is_the_prefix_and_twelve_hex_digits() {
        let id = referent_id("src/cli.ts", "probe");
        assert!(id.starts_with("veridoc:referent:"), "{id}");
        assert_eq!(id.len(), "veridoc:referent:".len() + 12, "{id}");
        assert!(id["veridoc:referent:".len()..]
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
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
        assert!(concept.starts_with("veridoc:concept:"), "{concept}");
        assert_ne!(concept, referent_id("configuration", "configuration"));
    }

    #[test]
    fn a_name_is_the_subject_name_and_the_predicate_slug() {
        assert_eq!(
            claim_name("probe", "rejects-missing-project-root"),
            "probe#rejects-missing-project-root"
        );
    }

    #[test]
    fn a_renamed_file_moves_no_claim_name() {
        // `0009`: no input to a claim's name or identity is a path, so the name a reader quoted
        // survives the move, and the referent id the report prints is the only thing that follows
        // the file.
        let old_file = referent_id("src/scanner.rs", "probe");
        let new_file = referent_id("src/scanner_v2.rs", "probe");
        assert_ne!(
            old_file, new_file,
            "the reported referent id follows the file"
        );
        assert_eq!(
            claim_name("probe", "rejects-missing-project-root"),
            "probe#rejects-missing-project-root"
        );
        for referent in [&old_file, &new_file] {
            assert_ne!(
                claim_identity("probe", "rejects-missing-project-root"),
                claim_identity(referent, "rejects-missing-project-root"),
                "the identity is no longer keyed on {referent}"
            );
        }
    }

    #[test]
    fn a_predicate_that_differs_only_in_case_and_punctuation_slugs_the_same() {
        assert_eq!(
            claim_name("probe", "Rejects Missing / PROJECT root"),
            claim_name("probe", "rejects-missing-project-root")
        );
    }

    #[test]
    fn a_long_predicate_is_cut_at_the_last_whole_word_that_fits() {
        assert_eq!(
            claim_name(
                "probe",
                "rejects a missing project root and then a second unreadable clause"
            ),
            "probe#rejects-a-missing-project-root-and-then-a-second"
        );
    }

    #[test]
    fn an_empty_predicate_names_the_subject_and_nothing_after_the_marker() {
        assert_eq!(claim_name("probe", ""), "probe#");
    }

    #[test]
    fn the_suffix_is_the_six_identity_digits_after_the_leading_c() {
        let identity = claim_identity("probe", "rejects-missing-project-root");
        assert!(identity.starts_with('c'), "{identity}");
        assert_eq!(suffix(&identity).len(), 7, "{identity}");
        assert_eq!(
            suffix(&identity),
            format!("~{}", &identity[1..7]),
            "the suffix is the identity's own digits"
        );
        assert_ne!(
            suffix(&identity),
            suffix(&claim_identity("probe", "rejects a missing project root")),
            "two predicates that slug alike get two suffixes"
        );
    }
}
