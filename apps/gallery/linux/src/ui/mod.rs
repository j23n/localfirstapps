//! Adaptive GTK shell.
//!
//! This `src/ui` tree is frozen as a reference: no new features, no
//! design-pass chrome, and no kit adoption here. New Linux Gallery work
//! belongs in `apps/gallery/ui-spec/`, `gallery-ffi` windows, and
//! `shells/gallery-gtk` (IMPLEMENTATION-PLAN Phase 5, GTK-DESIGN-PLAN
//! Phase 5).

mod cards;
mod thumbs;
mod window;

pub use window::Window;
