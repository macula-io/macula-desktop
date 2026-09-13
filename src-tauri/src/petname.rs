//! Petnames: the mesh-wide deterministic label for a node id, ported
//! from macula-mcp's own `src/petname.ts` (Docker-style
//! adjective_adjective_noun). The word lists and the derivation are
//! COPIED VERBATIM so every macula tool that shows a name shows the
//! same one -- the whole point of a petname is cross-tool stability,
//! and the function is a pure function of the node id (sha256 of its
//! lowercased hex, then three 16-bit reads modulo the list lengths).
//! A companion label, never a replacement: the real node_id stays next
//! to it everywhere, since only the real id is addressable.
//!
//! PROVENANCE: this is a stopgap copy. The function's proper home is
//! the SDK -- macula-io/macula-rust PR #1 adds
//! `macula_rust::petname::petname`. Once that ships in a published
//! crate version, THIS MODULE IS DELETED and every call site switches
//! to the SDK function.

use sha2::{Digest, Sha256};

const ADJECTIVES_A: [&str; 40] = [
    "bold", "bouncy", "brave", "breezy", "calm", "cheerful", "clever", "curious",
    "daring", "eager", "elegant", "fierce", "gentle", "graceful", "humble", "jolly",
    "jovial", "keen", "kind", "lively", "lucky", "mellow", "merry", "nimble",
    "noble", "plucky", "proud", "quiet", "quirky", "radiant", "silly", "sleepy",
    "spry", "sturdy", "tranquil", "upbeat", "vivid", "wise", "witty", "zealous",
];

const ADJECTIVES_B: [&str; 40] = [
    "amber", "azure", "bronze", "coral", "crimson", "cyan", "emerald", "golden",
    "green", "indigo", "ivory", "jade", "lavender", "lilac", "magenta", "maroon",
    "mauve", "navy", "olive", "orange", "peach", "pink", "plum", "purple",
    "red", "rust", "ruby", "sage", "salmon", "scarlet", "sienna", "silver",
    "slate", "tan", "teal", "turquoise", "violet", "yellow", "blue", "copper",
];

const NOUNS: [&str; 40] = [
    "antelope", "badger", "beetle", "bison", "cricket", "dolphin", "eagle", "elk",
    "falcon", "ferret", "flamingo", "fox", "gazelle", "gecko", "hare", "heron",
    "ibex", "iguana", "lynx", "marten", "mongoose", "moose", "narwhal", "orca",
    "otter", "owl", "panther", "pelican", "penguin", "rabbit", "raven", "salamander",
    "seal", "sparrow", "tiger", "toucan", "walrus", "weasel", "wolf", "wombat",
];

/// petname returns the stable "adjective_adjective_noun" label for a
/// node id: same input, same output, on every tool and every machine.
pub fn petname(node_id: &str) -> String {
    let digest = Sha256::digest(node_id.to_ascii_lowercase().as_bytes());
    let a = ADJECTIVES_A[u16::from_be_bytes([digest[0], digest[1]]) as usize % ADJECTIVES_A.len()];
    let b = ADJECTIVES_B[u16::from_be_bytes([digest[2], digest[3]]) as usize % ADJECTIVES_B.len()];
    let n = NOUNS[u16::from_be_bytes([digest[4], digest[5]]) as usize % NOUNS.len()];
    format!("{a}_{b}_{n}")
}

#[cfg(test)]
mod tests {
    use super::petname;

    #[test]
    fn petname_is_deterministic_and_shaped() {
        let id = "7374b0cfab4eea68e271c3815a0f78e21e913397f67345f337ddba7a3a88ab3a";
        let first = petname(id);
        assert_eq!(petname(id), first);
        let parts: Vec<&str> = first.split('_').collect();
        assert_eq!(parts.len(), 3, "adjective_adjective_noun");
    }
}
