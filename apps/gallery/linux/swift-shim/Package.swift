// swift-tools-version: 6.0
// Compile signal for the UniFFI Swift against a Linux-built libgallery_ffi.so.
// Not an app. `swift build` succeeding means the bindings type-check on Linux.
import PackageDescription

let package = Package(
    name: "GalleryFFICheck",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "gallery-ffi-check", targets: ["GalleryFFICheck"]),
    ],
    targets: [
        .systemLibrary(
            name: "GalleryCoreFFI",
            path: "CGalleryCoreFFI"
        ),
        .executableTarget(
            name: "GalleryFFICheck",
            dependencies: ["GalleryCoreFFI"],
            path: "Sources/GalleryFFICheck",
            swiftSettings: [
                // UniFFI emits top-level lets; Swift 6 treats that as script
                // code and then rejects `@main` unless every file is a library.
                .unsafeFlags(["-parse-as-library"]),
            ],
            linkerSettings: [
                .linkedLibrary("gallery_ffi"),
            ]
        ),
    ]
)
