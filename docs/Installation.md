Welcome to the official installation guide for PicoForge. This document will help you set up the application on your operating system.

## Windows

1.  #### Download the Installer

    Go to the [Latest Releases](https://github.com/librekeys/picoforge/releases/latest) page and download the `picoforge_[version]_[arch]-setup.exe` file.

2.  #### Run the Installer

    Locate the downloaded file and run it to install the application.

3.  #### Verify Smart Card Service

    For the application to communicate with your device, the Windows Smart Card service must be running. You can ensure this by running the following command in PowerShell:

    ```powershell
    Get-Service SCardSvr | Start-Service
    ```

> [!IMPORTANT]
> Please run the application as **Administrator**. This is often required for the application to correctly detect and interact with the hardware device.

## macOS

> [!WARNING]
> The macOS version of this application is tested rarely, while it does work fine, it might have some bugs that I have not experienced, feel free to report it on issues.
> 
> If you are a developer or user who can assist with testing and debugging on macOS, contributions are highly appreciated. Please check the Issues tab or join our Discord server.

#### Setup Instructions:

- Download the application from the [Latest Releases](https://github.com/librekeys/picoforge/releases/latest).
- No additional driver setup is usually required as the PC/SC framework is built into macOS.
  
> [!IMPORTANT]
> Note on Security: Picoforge images are currently unsigned. You will need to manually authorize the application to run.

1. Download the App: Get the latest version from the Releases Page.

  + Apple Silicon (M1/M2/M3): Download the arm64 version for native performance.
  + Intel Macs: Download the x64 version.

2. Authorization (Apple Silicon): Since the app is unsigned, choose one of the following methods to get it running on Apple Silicon:

#### Option A: The "Rosetta" Path (Easiest)

If you prefer a GUI approach, you can run the Intel version via translation.

+ Download the x64 image (e.g., picoforge_0.3.1_x64.dmg).
+ Open the file. If prompted, agree to install Rosetta.
+ Go to System Settings > Privacy & Security.
+ Scroll down to the Security section and click "Open Anyway" for Picoforge.

#### Option B: The "Terminal" Path (Recommended for Performance)

Use this to run the native arm64 version by stripping the "quarantine" flag.

+ Download and extract the arm64 zip file to find picoforge.app.
+ Open your Terminal and navigate to the folder containing the app.
+ Run the following command to clear the extended attributes:
 
```
xattr -cr picoforge.app
```

 + Right-click (or Control-click) the app and select Open.

## Linux

We offer multiple ways to run the application on Linux. Please choose the method for your distribution.

### Global Prerequisite: Smart Card Daemon

Regardless of the installation method you choose below, your **host operating system** must have the Smart Card Daemon (`pcscd`) installed and running. This service allows the application to talk to the USB device.

#### Enable pcscd on your host:

```bash
# Command may vary by distro, commonly:
sudo systemctl enable --now pcscd
```
To have the pcscd service, you may need to install pcsc-lite if it is not installed by default on your Linux distribution.
- On Debian : `sudo apt install pcscd`
- On NixOS, add this line in your /etc/nixos/configuration.nix : `services.pcscd.enable = true;`

### Quick Install (Recommended)

The fastest way to install PicoForge on any Linux distribution. The install script detects your architecture and libc, downloads the latest AppImage, and integrates it into your system with a desktop entry and icon.

You can inspect the script on GitHub before running: [`install.sh`](https://github.com/librekeys/picoforge/blob/main/install.sh)

```bash
curl -fsSL https://raw.githubusercontent.com/librekeys/picoforge/main/install.sh | sh
```

Or with `wget`:

```bash
wget -qO- https://raw.githubusercontent.com/librekeys/picoforge/main/install.sh | sh
```

To remove PicoForge later:

```bash
curl -fsSL https://raw.githubusercontent.com/librekeys/picoforge/main/install.sh | sh -s -- --uninstall
```

Run `sh install.sh --help` for all available options.

### Flathub

Available as a Flatpak on Flathub:

[![Get it on Flathub](https://flathub.org/api/badge?locale=en)](https://flathub.org/en-GB/apps/in.suyogtandel.picoforge)

```bash
flatpak install flathub in.suyogtandel.picoforge
```

### AppImage

AppImage is a self-contained format that runs on almost any Linux distribution without installation.

1. Download the `.AppImage` file from the [Latest Releases](https://github.com/librekeys/picoforge/releases/latest).
2. Mark it as executable:
   ```bash
   chmod +x picoforge_*.AppImage
   ```
3. Run it:
   ```bash
   ./picoforge_*.AppImage
   ```

> [!NOTE]
> If the AppImage does not start, you may be missing FUSE (Filesystem in Userspace), which is required on some newer distributions like Ubuntu 22.04+. Install `fuse3` via your package manager.

#### Auto-updates with Gear Lever

For automatic updates and desktop integration, use [Gear Lever](https://github.com/mijorus/gearlever). Install it via Flatpak:

```bash
flatpak install flathub it.mijorus.gearlever
```

Then open the downloaded `.AppImage` file with Gear Lever (or drag and drop it). It will manage updates and keep PicoForge in your application menu automatically.

### Fedora

[COPR](https://copr.fedorainfracloud.org/coprs/lockedmutex/picoforge/) is the recommended way of installing the application on Fedora.

#### Fedora Rawhide, 44, 43, 42

```bash
sudo dnf copr enable lockedmutex/picoforge
sudo dnf install picoforge
```

If you prefer to install the `.rpm` manually, download the `picoforge-*.el8.*.rpm` (available for `x86_64` and `aarch64`) from the [Latest Releases](https://github.com/librekeys/picoforge/releases/latest). This RPM is built on EPEL 8 and is compatible with Fedora and all EPEL-based distributions.

```bash
sudo dnf install ./picoforge-*.el8.*.rpm
```

### RHEL-based Distributions

#### RHEL, CentOS, AlmaLinux, Rocky Linux (EPEL 10, 9, 8)

```bash
sudo dnf install dnf-plugins-core
sudo dnf copr enable lockedmutex/picoforge
sudo dnf install picoforge
```

> [!NOTE]
> For EPEL 8, use `yum` instead of `dnf` if `dnf` is not available.

**Troubleshooting**: Ensure the EPEL repository is enabled on your system.

### openEuler

#### openEuler 24

PicoForge is available via COPR for openEuler 24.

```bash
sudo dnf copr enable lockedmutex/picoforge
sudo dnf install picoforge
```

### Post-Installation

After installation, you can launch picoforge from your application menu or run:

```bash
picoforge
```
