# Linux UniFFI compile signal

`swift build` of the generated `GalleryCore.swift` against a host-built
`libgallery_ffi.so`. Not an app. A type-check so the Fedora fleet sees
a broken FFI surface without waiting for `macos-26`.

```
# from apps/gallery
./scripts/generate_bindings.sh    # refresh Swift + header from the dylib
./linux/swift-shim/build.sh       # cargo build + swift build
```

Requires `swift` on PATH (`swift-lang` on Fedora, or a Swift.org toolchain)
and OpenSSL headers (`openssl-devel` / `libssl-dev`) for the host
`gallery-ffi` build.
