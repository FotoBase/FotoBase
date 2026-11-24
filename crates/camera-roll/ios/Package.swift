// swift-tools-version:5.5

import PackageDescription

let package = Package(
    name: "tauri-plugin-camera-roll",
    platforms: [
        .macOS(.v10_13),
        .iOS(.v15),
    ],
    products: [
        .library(
            name: "tauri-plugin-camera-roll",
            type: .static,
            targets: ["tauri-plugin-camera-roll"]),
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api")
    ],
    targets: [
        .target(
            name: "tauri-plugin-camera-roll",
            dependencies: [
                .byName(name: "Tauri")
            ],
            path: "Sources")
    ]
)
