//! The one place a path is deliberately normalized.
//!
//! Foundation has two spellings of "the path of this URL" and they are not the
//! same string:
//!
//! | expression | result |
//! |---|---|
//! | `url.path`, `url.standardized.path` | the **on-disk bytes**, untouched |
//! | `url.standardizedFileURL.path`, `resolvingSymlinksInPath().path` | **decomposed** (NFD) |
//!
//! NFD here is listing / prefix form only: failed-directory paths and the
//! Store's carry-forward prefix check, so both sides of *that* comparison
//! are NFD. Record identity is not this function — after M1 it is NFC via
//! `localcore-id` (Swift `precomposedStringWithCanonicalMapping`). NFC and
//! NFD spellings of one visible name no longer derive different ids.
//!
//! Each side is internally consistent. Mixing listing-form NFD with
//! identity-form NFC in a *string* compare is what breaks — and it breaks
//! invisibly in Swift, because `String ==` compares under canonical
//! equivalence and answers "equal" to a mismatch Rust's byte comparison would
//! catch. Hence: exactly one function, used for exactly the failed-directory
//! paths, and nothing else in this crate normalizes anything.

use unicode_normalization::UnicodeNormalization;

/// NFD, as `standardizedFileURL.path` produces.
pub fn decomposed(path: &str) -> String {
    path.nfd().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precomposed_names_decompose() {
        assert_eq!(decomposed("/lib/caf\u{e9}.jpg"), "/lib/cafe\u{301}.jpg");
    }

    #[test]
    fn already_decomposed_names_are_unchanged() {
        let nfd = "/lib/cafe\u{301}.jpg";
        assert_eq!(decomposed(nfd), nfd);
    }

    #[test]
    fn ascii_is_untouched() {
        assert_eq!(decomposed("/lib/Locked"), "/lib/Locked");
    }

    #[test]
    fn the_two_spellings_are_distinguishable_here_unlike_in_swift() {
        // Swift's `==` calls these equal; Rust's does not, which is why the
        // scanner fixture carries a `pathNormalization` token at all.
        assert_ne!("/lib/caf\u{e9}.jpg", "/lib/cafe\u{301}.jpg");
    }
}
