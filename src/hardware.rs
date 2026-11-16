//! What the PC is: the makers of its processor and graphics, the maker of the PC, laptop or desktop. Asked once
//! (PowerShell, CIM – a second or two), on its own thread so the window does not wait.

use std::sync::OnceLock;

#[derive(Clone, Debug, Default)]
pub struct Hardware {
    pub amd_cpu: bool,
    pub intel_cpu: bool,
    pub amd_gpu: bool,
    pub nvidia_gpu: bool,
    pub intel_gpu: bool,
    pub laptop: bool,
    /// the maker as Windows names it ("HP", "LENOVO", "ASUSTeK COMPUTER INC.", "Dell Inc."…)
    pub maker: String,
    /// the maker of the motherboard (a PC built from parts: "ASUSTeK COMPUTER INC.", "Micro-Star International"…)
    pub board: String,
    /// for the wizard: "AMD Ryzen 7 5800X3D · AMD Radeon RX 9070 XT · desktop"
    pub summary: String,
}

static FOUND: OnceLock<Hardware> = OnceLock::new();

/// Starts finding out (at the start of HeMimizer).
pub fn detect_in_background() {
    std::thread::spawn(|| {
        let _ = get();
    });
}

/// The hardware (finds it out on the first call).
pub fn get() -> &'static Hardware {
    FOUND.get_or_init(detect)
}

/// Known already (the wizard shows "Finding out…" until then).
pub fn ready() -> Option<&'static Hardware> {
    FOUND.get()
}

fn detect() -> Hardware {
    // one line per value, in this order: CPU, GPUs (joined by |), maker, chassis types, a battery, the motherboard
    let script = "$c=(Get-CimInstance Win32_Processor | Select-Object -First 1); \
        $g=(Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }) -join '|'; \
        $m=(Get-CimInstance Win32_ComputerSystem).Manufacturer; \
        $t=((Get-CimInstance Win32_SystemEnclosure).ChassisTypes) -join ','; \
        $b=@(Get-CimInstance Win32_Battery).Count; $bb=(Get-CimInstance Win32_BaseBoard).Manufacturer; \
        \"$($c.Manufacturer)`t$($c.Name)\"; $g; $m; $t; $b; $bb";
    let out = crate::sys::ps(script).unwrap_or_default();
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    let line = |i: usize| lines.get(i).copied().unwrap_or("");
    let (cpu_maker, cpu_name) = line(0).split_once('\t').unwrap_or((line(0), ""));
    let gpus: Vec<&str> = line(1).split('|').map(str::trim).filter(|g| !g.is_empty() && !g.contains("Basic Display") && !g.contains("Virtual")).collect();
    let has = |s: &str, k: &str| s.to_lowercase().contains(k);
    // laptop: a portable chassis (8–14, 30–32) or a battery
    let laptop = line(3).split(',').filter_map(|t| t.trim().parse::<u32>().ok()).any(|t| matches!(t, 8..=14 | 30..=32))
        || line(4).parse::<u32>().unwrap_or(0) > 0;
    let mut hw = Hardware {
        amd_cpu: has(cpu_maker, "amd"),
        intel_cpu: has(cpu_maker, "intel"),
        amd_gpu: gpus.iter().any(|g| has(g, "amd") || has(g, "radeon")),
        nvidia_gpu: gpus.iter().any(|g| has(g, "nvidia") || has(g, "geforce")),
        intel_gpu: gpus.iter().any(|g| has(g, "intel")),
        laptop,
        maker: line(2).to_string(),
        board: line(5).to_string(),
        summary: String::new(),
    };
    let mut parts: Vec<String> = Vec::new();
    if !cpu_name.trim().is_empty() {
        parts.push(cpu_name.trim().to_string());
    }
    parts.extend(gpus.iter().map(|g| g.to_string()));
    parts.push(if hw.laptop { "laptop".into() } else { "desktop".into() });
    hw.summary = parts.join(" · ");
    hw
}

impl Hardware {
    /// The drivers and the software of the maker the wizard ticks at first (catalog ids): NVIDIA graphics → the NVIDIA
    /// driver, an AMD processor → the AMD chipset driver, AMD graphics → the AMD graphics driver, Intel → the Intel
    /// drivers (when there is no NVIDIA / AMD graphics card, which bring their own).
    pub fn suggested(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.amd_cpu {
            v.push("amd-chipset");
        }
        if self.amd_gpu {
            v.push("amd-gpu");
        }
        if self.nvidia_gpu {
            v.push("nvidia");
        }
        if (self.intel_gpu || self.intel_cpu) && !self.nvidia_gpu && !self.amd_gpu {
            v.push("intel");
        }
        v.extend(self.maker_tools());
        v
    }

    /// The software of the maker (catalog ids): of a laptop or a ready-made PC by its maker (HP Support Assistant,
    /// Lenovo Vantage, MyASUS…); of a PC built from parts by the maker of its motherboard (Armoury Crate, MSI Center).
    pub fn maker_tools(&self) -> Vec<&'static str> {
        let m = self.maker.to_lowercase();
        let b = self.board.to_lowercase();
        let hp = m.contains("hewlett") || m == "hp" || m.starts_with("hp ");
        if hp {
            return vec!["hp"];
        }
        if m.contains("lenovo") {
            return vec!["lenovo"];
        }
        if m.contains("dell") || m.contains("alienware") {
            return vec!["dell"];
        }
        if m.contains("acer") {
            return vec!["acer"];
        }
        if self.laptop {
            if m.contains("asus") {
                return vec!["myasus"];
            }
            if m.contains("micro-star") || m.contains("msi") {
                return vec!["msi"];
            }
            return Vec::new();
        }
        // a desktop from parts: the motherboard decides
        if b.contains("asus") {
            vec!["armoury"]
        } else if b.contains("micro-star") || b == "msi" {
            vec!["msi"]
        } else {
            Vec::new()
        }
    }
}