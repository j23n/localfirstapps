# Mac workspace

Sibling of `docker/`. This side builds the iOS shells. The container
builds the portable halves (Rust, GTK, Go) and cannot run `xcodegen` or
`xcodebuild`.

```
./bootstrap.sh                      # once: Xcode CLT, rustup pin, XcodeGen 2.46.0
cd ../apps/gallery
./scripts/build_core.sh
xcodegen
open LocalGallery.xcodeproj
```

`bootstrap.sh` does not build. It fails naming the missing tool.

XcodeGen is installed by `apps/gallery/scripts/install_xcodegen.sh` at
the same pin CI uses (2.46.0). Do not `brew install xcodegen`.

Work-item routing: [`.agents/ROUTING.md`](../.agents/ROUTING.md). An
iOS-only edit still belongs to the work item, not to a path filter —
27% of gallery's recent commits touch Swift and `core/*.rs` together.
