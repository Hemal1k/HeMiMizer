# <img src="assets/HeMiMizer.png" width="40" height="40" alt=""> HeMiMizer

[![Latest release](https://img.shields.io/github/v/release/Hemal1k/HeMiMizer?label=release)](https://github.com/Hemal1k/HeMiMizer/releases/latest) [![Downloads](https://img.shields.io/github/downloads/Hemal1k/HeMiMizer/total)](https://github.com/Hemal1k/HeMiMizer/releases) [![License: MIT](https://img.shields.io/github/license/Hemal1k/HeMiMizer)](LICENSE) ![Windows 11](https://img.shields.io/badge/Windows-11-0078D4)

Gets a freshly installed Windows ready: the **drivers** for your hardware, the **programs** you choose, the **settings** of your
old PC and a **cleaner Windows** – in one go, after a short wizard.

**[Download](https://github.com/Hemal1k/HeMiMizer/releases/latest)** · [Project page](https://hemal1k.github.io/hemimizer.html) · [Report a problem](https://github.com/Hemal1k/HeMiMizer/issues)

![HeMiMizer](.github/screenshot.png)

> The downloads are not code-signed, so Windows SmartScreen may warn the first time (More info → Run anyway). Each release has `SHA256SUMS.txt` to check them.
> HeMiMizer changes Windows a lot and runs as administrator – use it on a new installation; the first step can be a restore point.

## How it works

1. **Before reinstalling:** start HeMiMizer on the old PC and choose **Back up this PC**. The wizard asks what the new Windows
   gets, and HeMiMizer saves it with your settings (and, if you like, all installers) into a folder – a USB stick or a network
   drive is best. HeMiMizer copies itself there too.
2. **After reinstalling:** start `HeMiMizer.exe` from that folder and choose **Set up this PC**. It installs the drivers, then
   the programs, puts the settings back and cleans Windows up – also across restarts.

Without a backup, **Set up this PC** runs the same wizard on the new PC.

## The wizard

| Page | What you choose |
|---|---|
| Drivers and the software of the maker | Ticked by the hardware found: NVIDIA graphics, AMD chipset / Radeon graphics, Intel drivers; for a laptop or a ready-made PC its maker's tool (HP Support Assistant, Lenovo Vantage, MyASUS, Dell, Acer, MSI), for a PC built from parts the tool of its motherboard (Armoury Crate, MSI Center). |
| Browser | One more besides Microsoft Edge (Chrome, Firefox, Brave, Opera, Vivaldi) – or none. |
| Programs | About 60 programs in sections, with a search: 7-Zip, VLC, Steam, Epic, Ubisoft Connect, EA app, Discord, Spotify, LibreOffice, VS Code, Git, Python, the free creative apps of ArtCraft… and HeMi Launcher. |
| Windows | Over 30 changes, each a switch, in groups: clean-up, privacy and AI, the taskbar and Start, File Explorer, system and games, updates. A starting point: recommended, like this PC (read from the old PC), or change nothing. |
| Settings to take along | From the old PC – Windows: wallpaper, colours, pinned apps, File Explorer, the mouse, Microsoft Defender, mapped network drives, Wi-Fi networks, fonts, display scaling; your programs: VLC, qBittorrent, Brave, Steam, Discord, Spotify, AMD Software, 7-Zip, Notepad++, PowerToys, OBS, VS Code with its extensions, Git… and programs found on the PC that HeMiMizer does not install (Adobe apps, DaVinci Resolve, Blender, JetBrains IDEs…). |
| Your folders | Own folders copied into the backup and put back to their place. |
| Where to save | The folder of the backup, the installers now (for a setup offline), a password for the backup, the name of the PC, and for each new program a shortcut on the desktop, pinned to Start or the taskbar, started with Windows – only what you tick. |

HeMiMizer does only what you choose or see ticked. A backup with a password is encrypted (AES-256-GCM): your settings, the
Wi-Fi passwords, the settings of programs and your folders – HeMiMizer asks for it on the new PC. After the backup it can
pack everything into one .zip. Before the setup runs, it shows what it will do; at the end, failed steps can be tried again,
and Done removes HeMiMizer from the PC – it writes no log, nothing of it stays.

## Tools

| Tool | What it does |
|---|---|
| Download Windows | The ISO of Windows 11 or 10 in the language you choose, straight from Microsoft's servers, with its progress – then a USB stick or a smaller Windows from it in one click. |
| Make a Windows USB stick | From a Windows 10 / 11 ISO, like Rufus: GPT (UEFI) or MBR (BIOS or UEFI), NTFS (a small FAT32 boot partition and an NTFS one – images of any size) or FAT32 with a split install.wim, the name from the ISO, and your choices for the installation – no TPM 2.0 / Secure Boot / RAM check, no Microsoft account required, a local account, the region and time zone you pick, no privacy questions, no automatic BitLocker. |
| Make a smaller Windows | A lighter Windows 11 ISO in the spirit of Tiny11, by HeMiMizer's own list: one edition, without ads, Teams, Clipchamp, Xbox, OneDrive and other extras, no TPM check, no Microsoft account needed – the simple choice, or choose yourself what goes and what stays; drivers and HeMiMizer can be added. |

Programs with their own licence terms are listed at the setup, each with a link: HeMiMizer installs them only after you
accept, or leaves them out.

The programs come through **winget** (the package manager of Windows), the HeMi apps from their GitHub releases, the AMD drivers
from amd.com. Nothing else is sent anywhere.

## Download

| File | For |
|---|---|
| `HeMiMizer.exe` | The program for x64 – one file, run it from any folder (it asks for administrator rights). |

Windows 11 (Windows 10 mostly works), x64 only. Also in [HeMi Launcher](https://github.com/Hemal1k/HeMiLauncher).

## Build

Rust (stable, MSVC). `cargo build --release` gives `build\release\HeMiMizer.exe`; `.\release.ps1` builds both downloads
(the GitHub workflow does the same for every release). A debug build runs without administrator rights, for trying the window.

| Folder | What |
|---|---|
| `src\app.rs` | The window: the first page, the wizard, the backup, the run and its result. |
| `src\catalog.rs`, `src\hardware.rs`, `src\plan.rs` | What the wizard offers, what the PC is, what was chosen (`plan.json` in the backup). |
| `src\install.rs` | Installs what was chosen: from the installers of the backup or through winget. |
| `src\steps.rs`, `src\windows.rs`, `src\debloat.rs`, `src\programs.rs`, `src\drivers.rs` | The steps of the setup. |
| `src\backup.rs`, `src\config.rs`, `src\appdata.rs` | The backup: settings, installers, own folders, the settings of programs. |
| `src\crypto.rs` | The backup with a password (AES-256-GCM, PBKDF2-SHA256). |
| `src\shortcuts.rs` | Desktop, Start, taskbar and the start with Windows of the new programs. |
| `src\usb.rs`, `src\iso.rs`, `src\msiso.rs` | The tools: a Windows USB stick, a smaller Windows ISO, the download of Windows from Microsoft. |

## License

[MIT](LICENSE). The parts of others are listed in `assets\ThirdPartyNotices.txt` (also in the program: Settings → Licences).
