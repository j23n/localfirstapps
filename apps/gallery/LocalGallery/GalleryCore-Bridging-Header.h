// C FFI for the Rust core. The header is committed next to this file
// (`GalleryCoreFFI.h`) and refreshed by `scripts/generate_bindings.sh`
// / `scripts/build_core.sh`. A bridging header (not the xcframework's
// GalleryCoreFFI clang module) is what Swift compiles against, so a
// stale DerivedData module cannot hide new symbols.
#include "GalleryCoreFFI.h"
