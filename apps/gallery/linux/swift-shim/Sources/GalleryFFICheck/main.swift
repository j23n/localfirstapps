// Type-check the generated bindings. Do not call into the Rust library —
// linking is enough of a signal that the symbols exist.
import GalleryCoreFFI

@main
enum GalleryFFICheck {
    static func main() {
        _ = String(cString: "ok")
    }
}
