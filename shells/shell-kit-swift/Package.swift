// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "shell-kit-swift",
    platforms: [
        .iOS(.v18),
        .macOS(.v15),
    ],
    products: [
        .library(name: "ShellKitSwift", targets: ["ShellKitSwift"]),
    ],
    targets: [
        .target(name: "ShellKitSwift"),
        .testTarget(
            name: "ShellKitSwiftTests",
            dependencies: ["ShellKitSwift"]
        ),
    ]
)
