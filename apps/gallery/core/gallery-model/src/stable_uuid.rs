//! Path-derived identity. Re-exports [`localcore_id::derive`], which
//! NFC-normalises UTF-8 path bytes before SHA-256 (ADR 0002 R4 / M1).
//! NFC is no longer a future question: it is the identity rule.

pub use localcore_id::derive;
