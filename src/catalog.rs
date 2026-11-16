//! What the wizard offers: drivers, the software of the maker, browsers, common programs and HeMi Launcher. Most come
//! through winget (also the Microsoft Store through its source msstore); HeMi Launcher from its releases on GitHub; the
//! AMD drivers from amd.com (or the installers next to HeMimizer.exe). `license`: the terms the user has to accept
//! (empty for open-source programs).

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Group {
    Driver,
    Maker,
    Browser,
    App,
    Hemi,
}

/// The sections of the programs.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Other,
    Essentials,
    Media,
    Games,
    Chat,
    Office,
    Developer,
    Creative,
}

pub const KINDS: &[(Kind, &str)] = &[
    (Kind::Essentials, "Essentials"),
    (Kind::Media, "Music, video and pictures"),
    (Kind::Games, "Games"),
    (Kind::Chat, "Chat, calls and e-mail"),
    (Kind::Office, "Office"),
    (Kind::Developer, "For developers"),
    (Kind::Creative, "ArtCraft – free creative apps"),
];

/// The icon of a section (Segoe Fluent Icons).
pub fn kind_glyph(k: Kind) -> char {
    match k {
        Kind::Essentials => '\u{E7B8}',
        Kind::Media => '\u{E8B2}',
        Kind::Games => '\u{E7FC}',
        Kind::Chat => '\u{E8BD}',
        Kind::Office => '\u{E8A5}',
        Kind::Developer => '\u{E943}',
        Kind::Creative => '\u{E790}',
        Kind::Other => '\u{E74C}',
    }
}

/// A line under the title of a section.
pub fn kind_note(k: Kind) -> &'static str {
    match k {
        Kind::Creative => "Open-source apps of getartcraft.com for pictures, video, sound and documents – still early versions",
        _ => "",
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Source {
    /// winget, the source winget (id)
    Winget(&'static str),
    /// winget, the source msstore (id of the Microsoft Store)
    Store(&'static str),
    /// the newest release of github.com/Hemal1k/<repo>: (repo, asset)
    GitHub(&'static str, &'static str),
    /// the own steps of HeMimizer (the AMD drivers)
    Own,
}

#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub id: &'static str,
    pub name: &'static str,
    pub line: &'static str,
    pub group: Group,
    pub kind: Kind,
    pub source: Source,
    /// the licence terms to accept (a web page); empty = open source, nothing to accept
    pub license: &'static str,
}

const fn it(id: &'static str, name: &'static str, line: &'static str, group: Group, kind: Kind, source: Source, license: &'static str) -> Item {
    Item { id, name, line, group, kind, source, license }
}

use Group::*;
use Kind::*;
use Source::*;

pub const ALL: &[Item] = &[
    // drivers – ticked by the hardware found (the user can change it)
    it("amd-chipset", "AMD chipset driver", "For AMD Ryzen processors", Driver, Other, Own, "https://www.amd.com/en/legal/eula/amd-software-eula.html"),
    it("amd-gpu", "AMD Radeon graphics driver", "AMD Software: Adrenalin Edition", Driver, Other, Own, "https://www.amd.com/en/legal/eula/amd-software-eula.html"),
    it("nvidia", "NVIDIA GeForce graphics driver", "Through the NVIDIA App", Driver, Other, Store("XP8CLZL93F5Z4P"), "https://www.nvidia.com/en-us/about-nvidia/eula-agreement/"),
    it("intel", "Intel drivers", "Processor, graphics and Wi-Fi – through the Intel Driver & Support Assistant", Driver, Other, Winget("Intel.IntelDriverAndSupportAssistant"), "https://www.intel.com/content/www/us/en/legal/terms-of-use.html"),
    // the software of the maker: of the laptop (or of a ready-made PC), else of the motherboard
    it("hp", "HP Support Assistant", "Drivers, BIOS and support for HP", Maker, Other, Winget("HP.SupportAssistant"), "https://www.hp.com/us-en/terms-of-use.html"),
    it("lenovo", "Lenovo Vantage", "Drivers, battery and settings for Lenovo", Maker, Other, Store("9WZDNCRFJ4MV"), "https://www.lenovo.com/us/en/legal/"),
    it("dell", "Dell Command | Update", "Drivers and BIOS for Dell", Maker, Other, Winget("Dell.CommandUpdate"), "https://www.dell.com/learn/us/en/uscorp1/terms-of-sale"),
    it("acer", "Acer Care Center S", "Drivers and support for Acer", Maker, Other, Store("9P8BB54NQNQ4"), "https://www.acer.com/us-en/privacy/terms-of-use"),
    it("myasus", "MyASUS", "Drivers and settings for ASUS laptops", Maker, Other, Store("9N7R5S6B0ZZH"), "https://www.asus.com/terms_of_use_notice_privacy_policy/official-site/"),
    it("armoury", "Armoury Crate", "Drivers, lighting and fans for ASUS motherboards", Maker, Other, Winget("Asus.ArmouryCrate"), "https://www.asus.com/terms_of_use_notice_privacy_policy/official-site/"),
    it("msi", "MSI Center", "Drivers, lighting and fans for MSI", Maker, Other, Store("9NVMNJCR03XV"), "https://www.msi.com/page/terms-of-use"),
    // browsers (one; Edge stays in Windows anyway)
    it("chrome", "Google Chrome", "", Browser, Other, Winget("Google.Chrome"), "https://www.google.com/chrome/terms/"),
    it("firefox", "Mozilla Firefox", "", Browser, Other, Winget("Mozilla.Firefox"), ""),
    it("brave", "Brave", "", Browser, Other, Winget("Brave.Brave"), ""),
    it("opera", "Opera", "", Browser, Other, Winget("Opera.Opera"), "https://legal.opera.com/eula/computers/"),
    it("vivaldi", "Vivaldi", "", Browser, Other, Winget("Vivaldi.Vivaldi"), "https://vivaldi.com/privacy/vivaldi-end-user-license-agreement/"),
    // programs
    it("7zip", "7-Zip", "Archives (zip, 7z, rar…)", App, Essentials, Winget("7zip.7zip"), ""),
    it("winrar", "WinRAR", "Archives – a trial, then paid", App, Essentials, Winget("RARLab.WinRAR"), "https://www.win-rar.com/winrarlicense.html"),
    it("powertoys", "PowerToys", "Tools of Microsoft for power users", App, Essentials, Winget("Microsoft.PowerToys"), ""),
    it("notepadpp", "Notepad++", "Text and code editor", App, Essentials, Winget("Notepad++.Notepad++"), ""),
    it("acrobat", "Adobe Acrobat Reader", "PDF reader", App, Essentials, Winget("Adobe.Acrobat.Reader.64-bit"), "https://www.adobe.com/legal/terms.html"),
    it("bitwarden", "Bitwarden", "Password manager", App, Essentials, Winget("Bitwarden.Bitwarden"), ""),
    it("keepassxc", "KeePassXC", "Password manager, offline", App, Essentials, Winget("KeePassXCTeam.KeePassXC"), ""),
    it("qbittorrent", "qBittorrent", "Torrents, without ads", App, Essentials, Winget("qBittorrent.qBittorrent"), ""),
    it("vlc", "VLC media player", "Plays every video and music file", App, Media, Winget("VideoLAN.VLC"), ""),
    it("spotify", "Spotify", "Music", App, Media, Winget("Spotify.Spotify"), "https://www.spotify.com/legal/end-user-agreement/"),
    it("obs", "OBS Studio", "Recording and streaming", App, Media, Winget("OBSProject.OBSStudio"), ""),
    it("sharex", "ShareX", "Screenshots and screen recording", App, Media, Winget("ShareX.ShareX"), ""),
    it("handbrake", "HandBrake", "Converts videos", App, Media, Winget("HandBrake.HandBrake"), ""),
    it("audacity", "Audacity", "Records and edits sound", App, Media, Winget("Audacity.Audacity"), ""),
    it("gimp", "GIMP", "Edits photos and pictures", App, Media, Winget("GIMP.GIMP"), ""),
    it("paintnet", "Paint.NET", "Simple picture editor", App, Media, Winget("dotPDN.PaintDotNet"), "https://getpaint.net/license.html"),
    it("steam", "Steam", "Games", App, Games, Winget("Valve.Steam"), "https://store.steampowered.com/subscriber_agreement/"),
    it("epic", "Epic Games Launcher", "Games", App, Games, Winget("EpicGames.EpicGamesLauncher"), "https://epicgames.com/eula"),
    it("gog", "GOG GALAXY", "Games", App, Games, Winget("GOG.Galaxy"), "https://www.gog.com/en/user_agreement"),
    it("ubisoft", "Ubisoft Connect", "Games of Ubisoft", App, Games, Winget("Ubisoft.Connect"), "https://legal.ubi.com/termsofuse/"),
    it("ea", "EA app", "Games of Electronic Arts", App, Games, Winget("ElectronicArts.EADesktop"), "https://tos.ea.com/legalapp/WEBTERMS/US/en/PC/"),
    it("discord", "Discord", "Chat and voice", App, Chat, Winget("Discord.Discord"), "https://discord.com/terms"),
    it("telegram", "Telegram", "Messages", App, Chat, Winget("Telegram.TelegramDesktop"), ""),
    it("whatsapp", "WhatsApp", "Messages and calls", App, Chat, Store("9NKSQGP7F2NH"), "https://www.whatsapp.com/legal/terms-of-service"),
    it("signal", "Signal", "Private messages", App, Chat, Winget("OpenWhisperSystems.Signal"), ""),
    it("zoom", "Zoom", "Video calls", App, Chat, Winget("Zoom.Zoom"), "https://explore.zoom.us/en/terms/"),
    it("teams", "Microsoft Teams", "Video calls and chat", App, Chat, Winget("Microsoft.Teams"), "https://www.microsoft.com/legal/terms-of-use"),
    it("slack", "Slack", "Team chat", App, Chat, Winget("SlackTechnologies.Slack"), "https://slack.com/terms-of-service/user"),
    it("thunderbird", "Thunderbird", "E-mail", App, Chat, Winget("Mozilla.Thunderbird"), ""),
    it("libreoffice", "LibreOffice", "Documents, spreadsheets, presentations", App, Office, Winget("TheDocumentFoundation.LibreOffice"), ""),
    it("vscode", "Visual Studio Code", "Code editor", App, Developer, Winget("Microsoft.VisualStudioCode"), "https://code.visualstudio.com/license"),
    it("git", "Git", "Version control", App, Developer, Winget("Git.Git"), ""),
    it("python", "Python 3.13", "Programming language", App, Developer, Winget("Python.Python.3.13"), ""),
    it("nodejs", "Node.js LTS", "JavaScript runtime", App, Developer, Winget("OpenJS.NodeJS.LTS"), ""),
    it("java", "Java 21 (Eclipse Temurin)", "Runs Java programs (Minecraft and others)", App, Developer, Winget("EclipseAdoptium.Temurin.21.JRE"), ""),
    // ArtCraft (getartcraft.com): open-source creative apps, early versions
    it("artcraft", "ArtCraft", "AI images and video, in one studio", App, Creative, Winget("ArtCraft.ArtCraft"), ""),
    it("photocraft", "PhotoCraft", "Image editor", App, Creative, Winget("ArtCraft.PhotoCraft"), ""),
    it("vectorcraft", "VectorCraft", "Vector illustration", App, Creative, Winget("ArtCraft.VectorCraft"), ""),
    it("filmcraft", "FilmCraft", "Video editor", App, Creative, Winget("ArtCraft.FilmCraft"), ""),
    it("lightcraft", "LightCraft", "Photo library and raw developer", App, Creative, Winget("ArtCraft.LightCraft"), ""),
    it("pdfcraft", "PdfCraft", "Reads, combines, splits and secures PDFs", App, Creative, Winget("ArtCraft.PdfCraft"), ""),
    it("effectcraft", "EffectCraft", "Motion graphics and visual effects", App, Creative, Winget("ArtCraft.EffectCraft"), ""),
    it("designcraft", "DesignCraft", "Page layout and publishing", App, Creative, Winget("ArtCraft.DesignCraft"), ""),
    it("texturecraft", "TextureCraft", "3D texture painting", App, Creative, Winget("ArtCraft.TextureCraft"), ""),
    it("materialcraft", "MaterialCraft", "Procedural materials with nodes", App, Creative, Winget("ArtCraft.MaterialCraft"), ""),
    it("soundcraft", "SoundCraft", "Records, edits and mixes audio", App, Creative, Winget("ArtCraft.SoundCraft"), ""),
    it("wordcraft", "WordCraft", "Word processor", App, Creative, Winget("ArtCraft.WordCraft"), ""),
    it("deckcraft", "DeckCraft", "Presentations", App, Creative, Winget("ArtCraft.DeckCraft"), ""),
    it("gridcraft", "GridCraft", "Spreadsheets and data analysis", App, Creative, Winget("ArtCraft.GridCraft"), ""),
    it("cadcraft", "CADCraft", "CAD and drafting", App, Creative, Winget("ArtCraft.CadCraft"), ""),
    it("craftanything", "Craft Anything", "Learn how software works and build your own", App, Creative, Winget("ArtCraft.CraftAnything"), ""),
    // the HeMi app offered: HeMi Launcher (it installs and updates the others)
    it("hemilauncher", "HeMi Launcher", "Installs and updates the free HeMi apps (a dock, a photo converter, a notepad…)", Hemi, Other, GitHub("HeMiLauncher", "HeMiLauncher-Setup.exe"), ""),
];

/// Programs that can be default apps (Windows does not let programs choose them): (catalog id, the part of the name
/// under which Windows lists it in Default apps).
pub const DEFAULT_APPS: &[(&str, &str)] = &[
    ("chrome", "Chrome"),
    ("firefox", "Firefox"),
    ("brave", "Brave"),
    ("opera", "Opera"),
    ("vivaldi", "Vivaldi"),
    ("vlc", "VLC"),
    ("7zip", "7-Zip"),
    ("winrar", "WinRAR"),
    ("qbittorrent", "qBittorrent"),
    ("acrobat", "Acrobat"),
    ("notepadpp", "Notepad++"),
    ("thunderbird", "Thunderbird"),
    ("libreoffice", "LibreOffice"),
];

/// The default apps to set by hand for the programs of the plan.
pub fn default_apps(plan: &crate::plan::Plan) -> Vec<&'static str> {
    DEFAULT_APPS.iter().filter(|(id, _)| plan.has(id)).map(|(_, n)| *n).collect()
}

pub fn find(id: &str) -> Option<&'static Item> {
    ALL.iter().find(|i| i.id == id)
}

pub fn of(group: Group) -> impl Iterator<Item = &'static Item> {
    ALL.iter().filter(move |i| i.group == group)
}

pub fn of_kind(kind: Kind) -> impl Iterator<Item = &'static Item> {
    ALL.iter().filter(move |i| i.group == Group::App && i.kind == kind)
}
