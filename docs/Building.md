# Building PicoForge All

Use current stable Rust. Clone this repository, install the dependencies below, then run `cargo build --release --locked`.

## Windows

Install Visual Studio Build Tools with **Desktop development with C++**, the Windows SDK, and the Rust MSVC toolchain. PC/SC is provided by Windows.

## macOS

Install Xcode Command Line Tools and Rust. PC/SC is provided by macOS.

## Linux

On Ubuntu/Debian:

```sh
sudo apt install build-essential pkg-config libpcsclite-dev pcscd libccid libudev-dev libvulkan-dev libwayland-dev wayland-protocols libxkbcommon-dev libxcb1-dev libxkbcommon-x11-dev libfontconfig1-dev libasound2-dev libdbus-1-dev libx11-dev libxcb-shape0-dev libxcb-xfixes0-dev libusb-1.0-0-dev
cargo build --release --locked
```

A graphical session and access to the security key are required to run the application. The repository also includes Nix development files.

## Tests

`cargo test --locked` runs unit tests. Tests marked `ignored` require hardware, external tools or explicit destructive-operation authorization; read each test before opting in.
