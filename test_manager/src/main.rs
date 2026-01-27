use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use sysinfo::{Disks, System};

#[cfg(all(feature = "nvidia", any(target_os = "windows", target_os = "linux")))]
use nvml_wrapper::Nvml;

// ================= Constants =================

const HISTORY_SIZE: usize = 120;
const FAST_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
const SLOW_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const INITIAL_CPU_SAMPLE_DELAY: Duration = Duration::from_millis(200);

const CARD_ROUNDING: f32 = 16.0;
const INNER_CARD_ROUNDING: f32 = 10.0;
const MAX_CONTENT_WIDTH: f32 = 1400.0;
const SIDE_PADDING: f32 = 40.0;

const TOP_PROCESS_COUNT: usize = 10;
const BYTES_PER_MB: f64 = 1_000_000.0;
const BYTES_PER_GB: f64 = 1_000_000_000.0;

const HIGH_USAGE_THRESHOLD: f32 = 85.0;
const MEDIUM_USAGE_THRESHOLD: f32 = 60.0;
const GPU_HIGH_TEMP_THRESHOLD: f32 = 80.0;
const GPU_MEDIUM_TEMP_THRESHOLD: f32 = 70.0;

const BATTERY_LOW_THRESHOLD: f32 = 20.0;
const BATTERY_MEDIUM_THRESHOLD: f32 = 50.0;

// ================= Theme =================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeMode {
    Dark,
    Light,
    Midnight,
    Nord,
}

#[derive(Debug, Clone, Copy)]
struct Theme {
    bg_primary: egui::Color32,
    bg_secondary: egui::Color32,
    bg_card: egui::Color32,
    bg_elevated: egui::Color32,
    accent: egui::Color32,
    text_primary: egui::Color32,
    text_secondary: egui::Color32,
    text_tertiary: egui::Color32,
    success: egui::Color32,
    warning: egui::Color32,
    danger: egui::Color32,
    border: egui::Color32,
}

impl Theme {
    fn from_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
            ThemeMode::Midnight => Self::midnight(),
            ThemeMode::Nord => Self::nord(),
        }
    }

    const fn dark() -> Self {
        Self {
            bg_primary: egui::Color32::from_rgb(17, 17, 19),
            bg_secondary: egui::Color32::from_rgb(22, 22, 24),
            bg_card: egui::Color32::from_rgb(27, 27, 30),
            bg_elevated: egui::Color32::from_rgb(35, 35, 38),
            accent: egui::Color32::from_rgb(88, 101, 242),
            text_primary: egui::Color32::from_rgb(245, 245, 247),
            text_secondary: egui::Color32::from_rgb(163, 163, 168),
            text_tertiary: egui::Color32::from_rgb(115, 115, 120),
            success: egui::Color32::from_rgb(52, 211, 153),
            warning: egui::Color32::from_rgb(251, 191, 36),
            danger: egui::Color32::from_rgb(248, 113, 113),
            border: egui::Color32::from_rgb(45, 45, 48),
        }
    }

    const fn midnight() -> Self {
        Self {
            bg_primary: egui::Color32::from_rgb(10, 12, 16),
            bg_secondary: egui::Color32::from_rgb(16, 18, 24),
            bg_card: egui::Color32::from_rgb(20, 22, 28),
            bg_elevated: egui::Color32::from_rgb(26, 28, 36),
            accent: egui::Color32::from_rgb(56, 189, 248),
            text_primary: egui::Color32::from_rgb(248, 250, 252),
            text_secondary: egui::Color32::from_rgb(148, 163, 184),
            text_tertiary: egui::Color32::from_rgb(100, 116, 139),
            success: egui::Color32::from_rgb(16, 185, 129),
            warning: egui::Color32::from_rgb(245, 158, 11),
            danger: egui::Color32::from_rgb(248, 113, 113),
            border: egui::Color32::from_rgb(38, 42, 54),
        }
    }

    const fn nord() -> Self {
        Self {
            bg_primary: egui::Color32::from_rgb(46, 52, 64),
            bg_secondary: egui::Color32::from_rgb(59, 66, 82),
            bg_card: egui::Color32::from_rgb(67, 76, 94),
            bg_elevated: egui::Color32::from_rgb(76, 86, 106),
            accent: egui::Color32::from_rgb(136, 192, 208),
            text_primary: egui::Color32::from_rgb(236, 239, 244),
            text_secondary: egui::Color32::from_rgb(216, 222, 233),
            text_tertiary: egui::Color32::from_rgb(143, 157, 177),
            success: egui::Color32::from_rgb(163, 190, 140),
            warning: egui::Color32::from_rgb(235, 203, 139),
            danger: egui::Color32::from_rgb(191, 97, 106),
            border: egui::Color32::from_rgb(81, 91, 112),
        }
    }

    const fn light() -> Self {
        Self {
            bg_primary: egui::Color32::from_rgb(250, 250, 252),
            bg_secondary: egui::Color32::from_rgb(241, 243, 247),
            bg_card: egui::Color32::from_rgb(255, 255, 255),
            bg_elevated: egui::Color32::from_rgb(248, 249, 251),
            accent: egui::Color32::from_rgb(79, 70, 229),
            text_primary: egui::Color32::from_rgb(17, 24, 39),
            text_secondary: egui::Color32::from_rgb(75, 85, 99),
            text_tertiary: egui::Color32::from_rgb(156, 163, 175),
            success: egui::Color32::from_rgb(16, 185, 129),
            warning: egui::Color32::from_rgb(245, 158, 11),
            danger: egui::Color32::from_rgb(239, 68, 68),
            border: egui::Color32::from_rgb(229, 231, 235),
        }
    }
}

// ================= Data Structures =================

#[derive(Debug, Clone)]
struct ProcessInfo {
    name: String,
    cpu_usage: f32,
    memory_bytes: u64,
}

impl ProcessInfo {
    #[inline]
    fn memory_mb(&self) -> f32 {
        self.memory_bytes as f32 / BYTES_PER_MB as f32
    }
}

#[derive(Debug, Clone)]
struct DiskInfo {
    mount_point: String,
    total_space: u64,
    available_space: u64,
    usage_percent: f32,
}

impl DiskInfo {
    #[inline]
    fn used_space(&self) -> u64 {
        self.total_space.saturating_sub(self.available_space)
    }
}

#[derive(Debug, Clone)]
struct CpuInfo {
    physical_cores: usize,
    logical_cores: usize,
    brand: String,
    base_frequency: Option<f32>,
    max_frequency: Option<f32>,
}

#[derive(Debug, Clone)]
struct CpuCoreStats {
    usage: f32,
    frequency: Option<f32>,
    temperature: Option<f32>,
}

#[derive(Debug, Clone)]
struct GpuInfo {
    name: String,
    utilization: f32,
    memory_used: u64,
    memory_total: u64,
    temperature: Option<f32>,
    power_usage: Option<f32>,
    is_integrated: bool,
}

impl GpuInfo {
    #[inline]
    fn memory_usage_percent(&self) -> f32 {
        if self.memory_total > 0 {
            (self.memory_used as f64 / self.memory_total as f64 * 100.0) as f32
        } else {
            0.0
        }
    }

    #[inline]
    fn memory_used_gb(&self) -> f64 {
        self.memory_used as f64 / BYTES_PER_GB
    }

    #[inline]
    fn memory_total_gb(&self) -> f64 {
        self.memory_total as f64 / BYTES_PER_GB
    }
}

#[derive(Debug, Clone)]
struct BatteryInfo {
    percentage: f32,
    is_charging: bool,
    time_remaining: Option<Duration>,
    power_consumption: Option<f32>,
    health: Option<f32>,
    capacity_wh: Option<f32>,
    energy_full_wh: Option<f32>,
}

#[derive(Debug, Clone)]
struct NpuInfo {
    name: String,
    utilization: f32,
    power_usage: Option<f32>,
    temperature: Option<f32>,
    frequency: Option<f32>,
    active_models: Vec<String>,
}

// ================= GPU Monitor =================

struct GpuMonitor {
    gpus: Vec<GpuInfo>,
    gpu_history: Vec<VecDeque<f32>>,
    last_gpu_times: Vec<u64>,
    last_update: Instant,
    #[cfg(all(feature = "nvidia", any(target_os = "windows", target_os = "linux")))]
    nvml: Option<Nvml>,
}

impl GpuMonitor {
    fn new() -> Self {
        #[cfg(all(feature = "nvidia", any(target_os = "windows", target_os = "linux")))]
        let nvml = Nvml::init().ok();

        Self {
            gpus: Vec::new(),
            gpu_history: Vec::new(),
            last_gpu_times: Vec::new(),
            last_update: Instant::now(),
            #[cfg(all(feature = "nvidia", any(target_os = "windows", target_os = "linux")))]
            nvml,
        }
    }

    fn refresh(&mut self) {
        self.gpus.clear();

        #[cfg(all(feature = "nvidia", any(target_os = "windows", target_os = "linux")))]
        if self.nvml.is_some() {
            self.refresh_nvidia_gpus();
        }

        #[cfg(target_os = "linux")]
        {
            self.refresh_amd_gpu_linux();
            self.refresh_intel_gpu_linux();
        }

        #[cfg(target_os = "windows")]
        self.refresh_intel_gpu_windows();

        if self.gpu_history.len() != self.gpus.len() {
            self.gpu_history
                .resize(self.gpus.len(), VecDeque::with_capacity(HISTORY_SIZE));
            self.last_gpu_times.resize(self.gpus.len(), 0);
        }

        for (i, gpu) in self.gpus.iter().enumerate() {
            if let Some(history) = self.gpu_history.get_mut(i) {
                if history.len() >= HISTORY_SIZE {
                    history.pop_front();
                }
                history.push_back(gpu.utilization);
            }
        }

        self.last_update = Instant::now();
    }

    #[cfg(target_os = "windows")]
    fn refresh_intel_gpu_windows(&mut self) {
        use std::process::Command;

        let output = Command::new("typeperf")
            .args(&[
                r"\GPU Engine(*engtype_3D)\Utilization Percentage",
                "-sc",
                "1",
            ])
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);

                for line in stdout.lines().skip(2) {
                    if line.to_lowercase().contains("intel") {
                        let parts: Vec<&str> = line.split(',').collect();
                        if parts.len() >= 2 {
                            if let Ok(util) = parts[1].trim().trim_matches('"').parse::<f32>() {
                                let util = util.clamp(0.0, 100.0);
                                if let Some(gpu) = self
                                    .gpus
                                    .iter_mut()
                                    .find(|g| g.name.contains("Intel") && g.is_integrated)
                                {
                                    gpu.utilization = util;
                                } else {
                                    self.gpus.push(GpuInfo {
                                        name: "Intel Integrated Graphics".to_string(),
                                        utilization: util,
                                        memory_used: 0,
                                        memory_total: 0,
                                        temperature: None,
                                        power_usage: None,
                                        is_integrated: true,
                                    });
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn refresh_intel_gpu_linux(&mut self) {
        use std::fs;
        use std::path::Path;

        let intel_gpu_paths = [
            "/sys/class/drm/card0",
            "/sys/class/drm/card1",
            "/sys/class/drm/card2",
        ];

        for card_path in &intel_gpu_paths {
            let path = Path::new(card_path);
            if !path.exists() {
                continue;
            }

            let vendor_path = path.join("device/vendor");
            if let Ok(vendor) = fs::read_to_string(&vendor_path) {
                if vendor.trim() != "0x8086" {
                    continue;
                }
            } else {
                continue;
            }

            let device_path = path.join("device");
            let device_id = fs::read_to_string(device_path.join("device"))
                .ok()
                .map(|s| s.trim().to_string());

            let name = if let Some(dev_id) = device_id.as_ref() {
                match dev_id.as_str() {
                    "0x7d45" | "0x7d55" | "0x7dd5" => {
                        "Intel Arc Graphics (Meteor Lake)".to_string()
                    }
                    _ => "Intel Integrated Graphics".to_string(),
                }
            } else {
                "Intel Integrated Graphics".to_string()
            };

            if self.gpus.iter().any(|g| g.name == name) {
                continue;
            }

            let mut utilization = 0.0;
            let time_delta = self.last_update.elapsed().as_secs_f32().max(0.01);

            let mut total_engine_ns = 0u64;

            if let Ok(proc_entries) = fs::read_dir("/proc") {
                for proc_entry in proc_entries.flatten() {
                    let fdinfo_path = proc_entry.path().join("fdinfo");
                    if let Ok(fdinfo_entries) = fs::read_dir(&fdinfo_path) {
                        for fdinfo_entry in fdinfo_entries.flatten() {
                            if let Ok(content) = fs::read_to_string(fdinfo_entry.path()) {
                                if content.contains("drm-driver:\ti915") || content.contains("i915")
                                {
                                    for line in content.lines() {
                                        if line.starts_with("drm-engine-render:")
                                            || line.starts_with("drm-engine-rcs0:")
                                            || line.starts_with("drm-engine-rcs:")
                                        {
                                            if let Some(time_str) = line.split(':').nth(1) {
                                                if let Some(ns_str) =
                                                    time_str.trim().split_whitespace().next()
                                                {
                                                    if let Ok(time_ns) = ns_str.parse::<u64>() {
                                                        total_engine_ns += time_ns;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let gpu_index = self.gpus.iter().filter(|g| g.is_integrated).count();

            if let Some(&last_time) = self.last_gpu_times.get(gpu_index) {
                if last_time > 0 && total_engine_ns > last_time {
                    let time_delta_ns = total_engine_ns - last_time;
                    let gpu_time_secs = time_delta_ns as f32 / 1_000_000_000.0;
                    utilization = (gpu_time_secs / time_delta * 100.0).clamp(0.0, 100.0);
                }
            }

            if self.last_gpu_times.len() > gpu_index {
                self.last_gpu_times[gpu_index] = total_engine_ns;
            } else {
                self.last_gpu_times.push(total_engine_ns);
            }

            if utilization < 5.0 {
                let gt_tile_path = device_path.join("gt/gt0");
                if gt_tile_path.exists() {
                    let freq_act_path = gt_tile_path.join("freq_act");

                    if let Ok(freq_act_str) = fs::read_to_string(&freq_act_path) {
                        if let Ok(act) = freq_act_str.trim().parse::<f32>() {
                            let min = fs::read_to_string(gt_tile_path.join("freq_min"))
                                .or_else(|_| {
                                    fs::read_to_string(gt_tile_path.join("rps_RPn_freq_mhz"))
                                })
                                .ok()
                                .and_then(|s| s.trim().parse::<f32>().ok())
                                .unwrap_or(300.0);

                            let max = fs::read_to_string(gt_tile_path.join("rps_boost_freq_mhz"))
                                .or_else(|_| {
                                    fs::read_to_string(gt_tile_path.join("rps_RP0_freq_mhz"))
                                })
                                .or_else(|_| fs::read_to_string(gt_tile_path.join("freq_max")))
                                .ok()
                                .and_then(|s| s.trim().parse::<f32>().ok())
                                .unwrap_or(2000.0);

                            if act > min + 50.0 && max > min {
                                let freq_util =
                                    ((act - min) / (max - min) * 100.0).clamp(0.0, 100.0);
                                utilization = utilization.max(freq_util);
                            }
                        }
                    }
                }
            }

            let mut memory_total = 0u64;
            let mut memory_used = 0u64;

            if let Ok(total_str) = fs::read_to_string(device_path.join("lmem_total_bytes")) {
                memory_total = total_str.trim().parse().unwrap_or(0);
            }

            if memory_total == 0 {
                if let Ok(mem_str) = fs::read_to_string(device_path.join("mem_info_vram_total")) {
                    memory_total = mem_str.trim().parse().unwrap_or(0);
                }
            }

            if let Ok(used_str) = fs::read_to_string(device_path.join("mem_info_vram_used")) {
                memory_used = used_str.trim().parse().unwrap_or(0);
            }

            if memory_used == 0 && memory_total > 0 && utilization > 10.0 {
                memory_used = ((memory_total as f32 * utilization / 100.0 * 0.3) as u64)
                    .max(512 * 1024 * 1024);
            }

            // For integrated GPUs, estimate memory if not available
            if memory_total == 0 {
                if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
                    for line in meminfo.lines() {
                        if line.starts_with("MemTotal:") {
                            if let Some(kb_str) = line.split_whitespace().nth(1) {
                                if let Ok(total_kb) = kb_str.parse::<u64>() {
                                    // Integrated GPUs typically can use up to 50% of system RAM
                                    memory_total = (total_kb * 1024) / 2;
                                    // Estimate current usage based on utilization
                                    if utilization > 5.0 {
                                        memory_used = ((memory_total as f32 * utilization / 100.0
                                            * 0.5)
                                            as u64)
                                            .max(256 * 1024 * 1024);
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            let mut temperature = None;
            let hwmon_path = device_path.join("hwmon");

            if let Ok(hwmon_entries) = fs::read_dir(&hwmon_path) {
                for hwmon_entry in hwmon_entries.flatten() {
                    let hwmon_dir = hwmon_entry.path();

                    for temp_num in 1..=5 {
                        let temp_input = hwmon_dir.join(format!("temp{}_input", temp_num));
                        if let Ok(temp_str) = fs::read_to_string(&temp_input) {
                            if let Ok(temp_millidegrees) = temp_str.trim().parse::<f32>() {
                                let temp_c = temp_millidegrees / 1000.0;
                                if (0.0..120.0).contains(&temp_c) {
                                    temperature = Some(temp_c);
                                    break;
                                }
                            }
                        }
                    }

                    if temperature.is_some() {
                        break;
                    }
                }
            }

            let mut power_usage = None;
            if let Ok(hwmon_entries) = fs::read_dir(&hwmon_path) {
                for hwmon_entry in hwmon_entries.flatten() {
                    let power_input = hwmon_entry.path().join("power1_input");
                    if let Ok(power_str) = fs::read_to_string(&power_input) {
                        if let Ok(power_microwatts) = power_str.trim().parse::<f32>() {
                            power_usage = Some(power_microwatts / 1_000_000.0);
                            break;
                        }
                    }
                }
            }

            self.gpus.push(GpuInfo {
                name,
                utilization,
                memory_used,
                memory_total,
                temperature,
                power_usage,
                is_integrated: true,
            });
        }
    }

    #[cfg(target_os = "linux")]
    fn refresh_amd_gpu_linux(&mut self) {
        use std::fs;

        if let Ok(entries) = fs::read_dir("/sys/class/drm") {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                if !name_str.starts_with("card")
                    || name_str.contains('-')
                    || name_str.contains("renderD")
                {
                    continue;
                }

                let device_path = path.join("device");
                let vendor_path = device_path.join("vendor");
                if let Ok(vendor) = fs::read_to_string(&vendor_path) {
                    if vendor.trim() != "0x1002" {
                        continue;
                    }
                } else {
                    continue;
                }

                let name = if let Ok(product_name) =
                    fs::read_to_string(device_path.join("product_name"))
                {
                    product_name.trim().to_string()
                } else {
                    format!("AMD GPU ({})", name_str)
                };

                if self.gpus.iter().any(|g| g.name == name) {
                    continue;
                }

                let gpu_busy_path = device_path.join("gpu_busy_percent");
                let utilization = if let Ok(busy_str) = fs::read_to_string(&gpu_busy_path) {
                    busy_str
                        .trim()
                        .parse::<f32>()
                        .unwrap_or(0.0)
                        .clamp(0.0, 100.0)
                } else {
                    0.0
                };

                let (memory_used, memory_total) = self.read_amd_vram_linux(&device_path);

                let temp_path = device_path.join("hwmon");
                let mut temperature = None;
                if let Ok(hwmon_entries) = fs::read_dir(&temp_path) {
                    for hwmon_entry in hwmon_entries.flatten() {
                        let temp_input = hwmon_entry.path().join("temp1_input");
                        if let Ok(temp_str) = fs::read_to_string(&temp_input) {
                            if let Ok(temp_millidegrees) = temp_str.trim().parse::<f32>() {
                                let temp_c = temp_millidegrees / 1000.0;
                                if (0.0..120.0).contains(&temp_c) {
                                    temperature = Some(temp_c);
                                    break;
                                }
                            }
                        }
                    }
                }

                let mut power_usage = None;
                if let Ok(hwmon_entries) = fs::read_dir(&temp_path) {
                    for hwmon_entry in hwmon_entries.flatten() {
                        let power_input = hwmon_entry.path().join("power1_average");
                        if let Ok(power_str) = fs::read_to_string(&power_input) {
                            if let Ok(power_microwatts) = power_str.trim().parse::<f32>() {
                                power_usage = Some(power_microwatts / 1_000_000.0);
                                break;
                            }
                        }
                    }
                }

                let memory_gb = memory_total as f64 / BYTES_PER_GB;
                let is_integrated = memory_gb < 4.0 || name.to_lowercase().contains("integrated");

                self.gpus.push(GpuInfo {
                    name,
                    utilization,
                    memory_used,
                    memory_total,
                    temperature,
                    power_usage,
                    is_integrated,
                });
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn read_amd_vram_linux(&self, device_path: &std::path::Path) -> (u64, u64) {
        use std::fs;

        let vram_used_path = device_path.join("mem_info_vram_used");
        let vram_total_path = device_path.join("mem_info_vram_total");

        let used = fs::read_to_string(&vram_used_path)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);

        let total = fs::read_to_string(&vram_total_path)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);

        (used, total)
    }

    #[cfg(all(feature = "nvidia", any(target_os = "windows", target_os = "linux")))]
    fn refresh_nvidia_gpus(&mut self) {
        let nvml = match self.nvml.as_ref() {
            Some(n) => n,
            None => return,
        };

        let device_count = match nvml.device_count() {
            Ok(count) => count,
            Err(_) => return,
        };

        for i in 0..device_count {
            if let Ok(device) = nvml.device_by_index(i) {
                let name = device
                    .name()
                    .unwrap_or_else(|_| format!("NVIDIA GPU {}", i));

                let name_lower = name.to_lowercase();
                let is_integrated_by_name = name_lower.contains("integrated")
                    || name_lower.contains("igpu")
                    || name_lower.contains("uhd")
                    || name_lower.contains("iris");

                let (memory_used, memory_total) = device
                    .memory_info()
                    .map(|m| (m.used, m.total))
                    .unwrap_or((0, 1));
                let memory_gb = memory_total as f64 / BYTES_PER_GB;
                let is_integrated = is_integrated_by_name || memory_gb < 2.0;

                let utilization = device
                    .utilization_rates()
                    .ok()
                    .map(|u| (u.gpu as f32).clamp(0.0, 100.0))
                    .unwrap_or(0.0);
                let temperature = device
                    .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
                    .ok()
                    .map(|t| t as f32);
                let power_usage = device.power_usage().ok().map(|p| p as f32 / 1000.0);

                self.gpus.push(GpuInfo {
                    name,
                    utilization,
                    memory_used,
                    memory_total,
                    temperature,
                    power_usage,
                    is_integrated,
                });
            }
        }
    }

    #[inline]
    fn has_discrete_gpu(&self) -> bool {
        self.gpus.iter().any(|gpu| !gpu.is_integrated)
    }

    #[inline]
    fn has_integrated_gpu(&self) -> bool {
        self.gpus.iter().any(|gpu| gpu.is_integrated)
    }

    fn get_discrete_gpus(&self) -> Vec<&GpuInfo> {
        self.gpus.iter().filter(|gpu| !gpu.is_integrated).collect()
    }

    fn get_integrated_gpus(&self) -> Vec<&GpuInfo> {
        self.gpus.iter().filter(|gpu| gpu.is_integrated).collect()
    }
}

// ================= Battery Monitor =================

struct BatteryMonitor {
    manager: Option<battery::Manager>,
    battery_info: Option<BatteryInfo>,
    charge_history: VecDeque<f32>,
}

impl BatteryMonitor {
    fn new() -> Self {
        let manager = battery::Manager::new().ok();
        Self {
            manager,
            battery_info: None,
            charge_history: VecDeque::with_capacity(HISTORY_SIZE),
        }
    }

    fn refresh(&mut self) {
        if let Some(ref manager) = self.manager {
            if let Ok(mut batteries) = manager.batteries() {
                if let Some(Ok(battery)) = batteries.next() {
                    let percentage = battery
                        .state_of_charge()
                        .get::<battery::units::ratio::percent>();
                    let is_charging = matches!(battery.state(), battery::State::Charging);

                    let time_remaining = if is_charging {
                        battery.time_to_full()
                    } else {
                        battery.time_to_empty()
                    }
                    .and_then(|time| {
                        let seconds = time.get::<battery::units::time::second>() as u64;
                        if seconds > 0 {
                            Some(Duration::from_secs(seconds))
                        } else {
                            None
                        }
                    });

                    let power_consumption = if battery.energy_rate().value > 0.0 {
                        Some(battery.energy_rate().get::<battery::units::power::watt>())
                    } else {
                        None
                    };

                    let health = if battery.energy_full_design().value > 0.0 {
                        Some(
                            (battery.energy_full().value / battery.energy_full_design().value
                                * 100.0) as f32,
                        )
                    } else {
                        None
                    };

                    let capacity_wh = if battery.energy().value > 0.0 {
                        Some(battery.energy().get::<battery::units::energy::watt_hour>())
                    } else {
                        None
                    };

                    let energy_full_wh = if battery.energy_full().value > 0.0 {
                        Some(
                            battery
                                .energy_full()
                                .get::<battery::units::energy::watt_hour>(),
                        )
                    } else {
                        None
                    };

                    self.battery_info = Some(BatteryInfo {
                        percentage,
                        is_charging,
                        time_remaining,
                        power_consumption,
                        health,
                        capacity_wh,
                        energy_full_wh,
                    });

                    if self.charge_history.len() >= HISTORY_SIZE {
                        self.charge_history.pop_front();
                    }
                    self.charge_history.push_back(percentage);
                }
            }
        }
    }

    #[inline]
    fn has_battery(&self) -> bool {
        self.battery_info.is_some()
    }
}

// ================= NPU Monitor =================

struct NpuMonitor {
    npu_info: Option<NpuInfo>,
    npu_history: VecDeque<f32>,
}

impl NpuMonitor {
    fn new() -> Self {
        Self {
            npu_info: None,
            npu_history: VecDeque::with_capacity(HISTORY_SIZE),
        }
    }

    fn refresh(&mut self) {
        #[cfg(target_os = "linux")]
        self.refresh_intel_npu_linux();

        #[cfg(target_os = "windows")]
        self.refresh_intel_npu_windows();

        if let Some(npu) = &self.npu_info {
            if self.npu_history.len() >= HISTORY_SIZE {
                self.npu_history.pop_front();
            }
            self.npu_history.push_back(npu.utilization);
        }
    }

    #[cfg(target_os = "linux")]
    fn refresh_intel_npu_linux(&mut self) {
        use std::fs;

        let mut npu_detected = false;
        let mut npu_base_path: Option<std::path::PathBuf> = None;

        // Check /sys/class/accel first
        if let Ok(entries) = fs::read_dir("/sys/class/accel") {
            for entry in entries.flatten() {
                let path = entry.path();
                let device_path = path.join("device");
                if let Ok(vendor) = fs::read_to_string(device_path.join("vendor")) {
                    if vendor.trim() == "0x8086" {
                        npu_detected = true;
                        npu_base_path = Some(path);
                        break;
                    }
                }
            }
        }

        // Check PCI devices
        if !npu_detected {
            let pci_paths = [
                "/sys/devices/pci0000:00/0000:00:0b.0",
                "/sys/devices/pci0000:00/0000:00:08.0",
                "/sys/bus/pci/drivers/intel_vpu",
            ];

            for pci_path in &pci_paths {
                let path = std::path::Path::new(pci_path);
                if path.exists() {
                    if let Ok(vendor) = fs::read_to_string(path.join("vendor")) {
                        if vendor.trim() == "0x8086" {
                            npu_detected = true;
                            npu_base_path = Some(path.to_path_buf());
                            break;
                        }
                    }
                }
            }
        }

        // Check kernel modules
        if !npu_detected {
            if let Ok(modules) = fs::read_to_string("/proc/modules") {
                if modules.contains("intel_vpu") || modules.contains("intel_npu") {
                    npu_detected = true;
                }
            }
        }

        // Check for DRM render nodes
        if !npu_detected {
            if let Ok(entries) = fs::read_dir("/dev/dri") {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("renderD") {
                        // Try to identify if this is an NPU device
                        if let Ok(realpath) = fs::read_link(entry.path()) {
                            let path_str = realpath.to_string_lossy();
                            if path_str.contains("accel") || path_str.contains("vpu") {
                                npu_detected = true;
                                break;
                            }
                        }
                    }
                }
            }
        }

        if !npu_detected {
            self.npu_info = None;
            return;
        }

        let name = "Intel AI Boost (NPU)".to_string();

        let mut utilization = 0.0;
        let mut active_models = Vec::new();

        // Check for active NPU processes
        if let Ok(proc_entries) = fs::read_dir("/proc") {
            let mut npu_process_count = 0;

            for proc_entry in proc_entries.flatten() {
                let proc_name = proc_entry.file_name();
                let proc_str = proc_name.to_string_lossy();

                if !proc_str.chars().all(|c| c.is_numeric()) {
                    continue;
                }

                let fdinfo_path = proc_entry.path().join("fdinfo");
                if let Ok(fdinfo_entries) = fs::read_dir(&fdinfo_path) {
                    let mut found_npu_in_proc = false;

                    for fdinfo_entry in fdinfo_entries.flatten() {
                        if let Ok(content) = fs::read_to_string(fdinfo_entry.path()) {
                            let has_vpu = content.contains("intel_vpu")
                                || content.contains("intel-vpu")
                                || content.contains("intel_npu")
                                || content.contains("drm-driver:\taccel")
                                || content.contains("accel/accel");

                            if has_vpu && !found_npu_in_proc {
                                found_npu_in_proc = true;
                                npu_process_count += 1;

                                if let Ok(cmdline) =
                                    fs::read_to_string(proc_entry.path().join("cmdline"))
                                {
                                    let process_name = cmdline
                                        .split('\0')
                                        .next()
                                        .and_then(|p| p.split('/').last())
                                        .filter(|s| !s.is_empty())
                                        .unwrap_or("unknown");

                                    if !active_models.contains(&process_name.to_string()) {
                                        active_models.push(process_name.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if npu_process_count > 0 {
                utilization = (npu_process_count as f32 * 30.0).min(95.0);
            }
        }

        let mut frequency = None;
        let mut temperature = None;
        let mut power_usage = None;

        if let Some(base_path) = npu_base_path.as_ref() {
            let freq_paths = [
                base_path.join("device/current_frequency"),
                base_path.join("device/cur_freq"),
                base_path.join("gt/gt0/freq_act"),
            ];

            frequency = freq_paths
                .iter()
                .find_map(|p| fs::read_to_string(p).ok())
                .and_then(|s| s.trim().parse::<f32>().ok())
                .map(|hz| hz / 1_000_000.0);

            let hwmon_paths = [
                base_path.join("device/hwmon"),
                base_path.join("hwmon"),
                base_path.join("device/device/hwmon"),
            ];

            for hwmon_path in &hwmon_paths {
                if let Ok(hwmon_entries) = fs::read_dir(hwmon_path) {
                    for hwmon_entry in hwmon_entries.flatten() {
                        for temp_num in 1..=5 {
                            let temp_input =
                                hwmon_entry.path().join(format!("temp{}_input", temp_num));
                            if let Ok(temp_str) = fs::read_to_string(&temp_input) {
                                if let Ok(temp_milli) = temp_str.trim().parse::<f32>() {
                                    let temp_c = temp_milli / 1000.0;
                                    if (0.0..120.0).contains(&temp_c) {
                                        temperature = Some(temp_c);
                                        break;
                                    }
                                }
                            }
                        }
                        if temperature.is_some() {
                            break;
                        }
                    }
                    if temperature.is_some() {
                        break;
                    }
                }
            }

            for hwmon_path in &hwmon_paths {
                if let Ok(hwmon_entries) = fs::read_dir(hwmon_path) {
                    for hwmon_entry in hwmon_entries.flatten() {
                        let power_paths = [
                            hwmon_entry.path().join("power1_input"),
                            hwmon_entry.path().join("power1_average"),
                        ];

                        for power_path in &power_paths {
                            if let Ok(power_str) = fs::read_to_string(power_path) {
                                if let Ok(power_microwatts) = power_str.trim().parse::<f32>() {
                                    power_usage = Some(power_microwatts / 1_000_000.0);
                                    break;
                                }
                            }
                        }
                        if power_usage.is_some() {
                            break;
                        }
                    }
                    if power_usage.is_some() {
                        break;
                    }
                }
            }
        }

        // Always create NPU info if detected, even with 0% utilization
        self.npu_info = Some(NpuInfo {
            name,
            utilization,
            power_usage,
            temperature,
            frequency,
            active_models,
        });
    }

    #[cfg(target_os = "windows")]
    fn refresh_intel_npu_windows(&mut self) {
        // Windows NPU detection not implemented
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    fn refresh_intel_npu_linux(&mut self) {}

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    fn refresh_intel_npu_windows(&mut self) {}

    #[inline]
    fn has_npu(&self) -> bool {
        self.npu_info.is_some()
    }
}

// ================= System Monitor =================

struct SystemMonitor {
    system: System,
    disks: Disks,
    cpu_history: Vec<VecDeque<f32>>,
    memory_history: VecDeque<f32>,
    top_processes: Vec<ProcessInfo>,
    total_processes: usize,
    disk_stats: Vec<DiskInfo>,
    cpu_info: CpuInfo,
    cpu_core_stats: Vec<CpuCoreStats>,
    cpu_package_temp: Option<f32>,
    gpu_monitor: GpuMonitor,
    npu_monitor: NpuMonitor,
    battery_monitor: BatteryMonitor,
    last_fast_refresh: Instant,
    last_slow_refresh: Instant,
    theme_mode: ThemeMode,
    theme: Theme,
}

impl SystemMonitor {
    fn new() -> Self {
        let mut system = System::new_all();
        std::thread::sleep(INITIAL_CPU_SAMPLE_DELAY);
        system.refresh_cpu_all();
        system.refresh_memory();

        let cpu_count = system.cpus().len();
        let physical_cores = system.physical_core_count().unwrap_or(cpu_count);

        let cpu_brand = system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().to_string())
            .unwrap_or_else(|| "Unknown CPU".to_string());

        let (base_freq, max_freq) = Self::read_cpu_frequencies();

        let cpu_info = CpuInfo {
            physical_cores,
            logical_cores: cpu_count,
            brand: cpu_brand,
            base_frequency: base_freq,
            max_frequency: max_freq,
        };

        let disks = Disks::new_with_refreshed_list();
        let theme_mode = ThemeMode::Dark;

        Self {
            system,
            disks,
            cpu_history: vec![VecDeque::with_capacity(HISTORY_SIZE); cpu_count],
            memory_history: VecDeque::with_capacity(HISTORY_SIZE),
            top_processes: Vec::new(),
            total_processes: 0,
            disk_stats: Vec::new(),
            cpu_info,
            cpu_core_stats: vec![
                CpuCoreStats {
                    usage: 0.0,
                    frequency: None,
                    temperature: None
                };
                cpu_count
            ],
            cpu_package_temp: None,
            gpu_monitor: GpuMonitor::new(),
            npu_monitor: NpuMonitor::new(),
            battery_monitor: BatteryMonitor::new(),
            last_fast_refresh: Instant::now(),
            last_slow_refresh: Instant::now(),
            theme_mode,
            theme: Theme::from_mode(theme_mode),
        }
    }

    #[cfg(target_os = "linux")]
    fn read_cpu_frequencies() -> (Option<f32>, Option<f32>) {
        use std::fs;

        let base_freq = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/base_frequency")
            .or_else(|_| {
                fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq")
            })
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
            .map(|khz| khz / 1_000_000.0);

        let max_freq = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
            .map(|khz| khz / 1_000_000.0);

        (base_freq, max_freq)
    }

    #[cfg(not(target_os = "linux"))]
    fn read_cpu_frequencies() -> (Option<f32>, Option<f32>) {
        (None, None)
    }

    fn refresh_fast_metrics(&mut self) {
        self.system.refresh_cpu_all();
        self.system.refresh_memory();

        for (i, cpu) in self.system.cpus().iter().enumerate() {
            let usage = cpu.cpu_usage().clamp(0.0, 100.0);
            if let Some(history) = self.cpu_history.get_mut(i) {
                if history.len() >= HISTORY_SIZE {
                    history.pop_front();
                }
                history.push_back(usage);
            }

            if let Some(core_stat) = self.cpu_core_stats.get_mut(i) {
                core_stat.usage = usage;
                core_stat.frequency = Self::read_core_frequency(i);
                core_stat.temperature = Self::read_core_temperature(i);
            }
        }

        self.cpu_package_temp = Self::read_package_temperature();

        let mem_usage = self.memory_usage_percent();
        if self.memory_history.len() >= HISTORY_SIZE {
            self.memory_history.pop_front();
        }
        self.memory_history.push_back(mem_usage);
    }

    #[cfg(target_os = "linux")]
    fn read_core_frequency(core: usize) -> Option<f32> {
        use std::fs;
        let path = format!(
            "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq",
            core
        );
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
            .map(|khz| khz / 1_000_000.0)
    }

    #[cfg(not(target_os = "linux"))]
    fn read_core_frequency(_core: usize) -> Option<f32> {
        None
    }

    #[cfg(target_os = "linux")]
    fn read_core_temperature(core: usize) -> Option<f32> {
        use std::fs;
        let hwmon_path = "/sys/devices/platform/coretemp.0/hwmon";
        if let Ok(hwmon_entries) = fs::read_dir(hwmon_path) {
            for hwmon_entry in hwmon_entries.flatten() {
                let temp_input_path = hwmon_entry.path().join(format!("temp{}_input", core + 2));

                if temp_input_path.exists() {
                    if let Ok(temp_str) = fs::read_to_string(&temp_input_path) {
                        if let Ok(temp_milli) = temp_str.trim().parse::<f32>() {
                            let temp_c = temp_milli / 1000.0;
                            if (0.0..120.0).contains(&temp_c) {
                                return Some(temp_c);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    #[cfg(not(target_os = "linux"))]
    fn read_core_temperature(_core: usize) -> Option<f32> {
        None
    }

    #[cfg(target_os = "linux")]
    fn read_package_temperature() -> Option<f32> {
        use std::fs;

        let hwmon_path = "/sys/devices/platform/coretemp.0/hwmon";
        if let Ok(hwmon_entries) = fs::read_dir(hwmon_path) {
            for hwmon_entry in hwmon_entries.flatten() {
                let temp_input_path = hwmon_entry.path().join("temp1_input");
                if temp_input_path.exists() {
                    if let Ok(temp_str) = fs::read_to_string(&temp_input_path) {
                        if let Ok(temp_milli) = temp_str.trim().parse::<f32>() {
                            let temp_c = temp_milli / 1000.0;
                            if (0.0..120.0).contains(&temp_c) {
                                return Some(temp_c);
                            }
                        }
                    }
                }
            }
        }

        if let Ok(thermal_entries) = fs::read_dir("/sys/class/thermal") {
            for thermal_entry in thermal_entries.flatten() {
                let type_path = thermal_entry.path().join("type");
                if let Ok(thermal_type) = fs::read_to_string(&type_path) {
                    if thermal_type.trim() == "x86_pkg_temp" {
                        let temp_path = thermal_entry.path().join("temp");
                        if let Ok(temp_str) = fs::read_to_string(&temp_path) {
                            if let Ok(temp_milli) = temp_str.trim().parse::<f32>() {
                                let temp_c = temp_milli / 1000.0;
                                if (0.0..120.0).contains(&temp_c) {
                                    return Some(temp_c);
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }

    #[cfg(not(target_os = "linux"))]
    fn read_package_temperature() -> Option<f32> {
        None
    }

    fn refresh_slow_metrics(&mut self) {
        self.refresh_process_stats();
        self.refresh_disk_stats();
        self.gpu_monitor.refresh();
        self.npu_monitor.refresh();
        self.battery_monitor.refresh();
    }

    fn refresh_process_stats(&mut self) {
        self.system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::new().with_cpu().with_memory(),
        );

        let mut processes: Vec<ProcessInfo> = self
            .system
            .processes()
            .values()
            .map(|process| ProcessInfo {
                name: process.name().to_string_lossy().into_owned(),
                cpu_usage: process.cpu_usage(),
                memory_bytes: process.memory(),
            })
            .collect();

        processes.sort_unstable_by(|a, b| {
            b.cpu_usage
                .partial_cmp(&a.cpu_usage)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        processes.truncate(TOP_PROCESS_COUNT);
        self.total_processes = self.system.processes().len();
        self.top_processes = processes;
    }

    fn refresh_disk_stats(&mut self) {
        self.disks.refresh_list();
        self.disk_stats = self
            .disks
            .iter()
            .map(|disk| {
                let total = disk.total_space();
                let available = disk.available_space();
                let used = total.saturating_sub(available);
                let usage_percent = if total > 0 {
                    (used as f64 / total as f64 * 100.0) as f32
                } else {
                    0.0
                };

                DiskInfo {
                    mount_point: disk.mount_point().to_string_lossy().to_string(),
                    total_space: total,
                    available_space: available,
                    usage_percent,
                }
            })
            .collect();
    }

    #[inline]
    fn avg_cpu_usage(&self) -> f32 {
        if self.cpu_history.is_empty() {
            return 0.0;
        }
        let sum: f32 = self
            .cpu_history
            .iter()
            .filter_map(|h| h.back().copied())
            .sum();
        sum / self.cpu_history.len() as f32
    }

    #[inline]
    fn memory_usage_percent(&self) -> f32 {
        let total = self.system.total_memory();
        if total == 0 {
            return 0.0;
        }
        (self.system.used_memory() as f32 / total as f32) * 100.0
    }

    #[inline]
    fn memory_used_gb(&self) -> f64 {
        self.system.used_memory() as f64 / BYTES_PER_GB
    }

    #[inline]
    fn memory_total_gb(&self) -> f64 {
        self.system.total_memory() as f64 / BYTES_PER_GB
    }

    #[inline]
    fn get_usage_color(&self, usage: f32) -> egui::Color32 {
        if usage >= HIGH_USAGE_THRESHOLD {
            self.theme.danger
        } else if usage >= MEDIUM_USAGE_THRESHOLD {
            self.theme.warning
        } else {
            self.theme.success
        }
    }

    #[inline]
    fn get_temp_color(&self, temp: f32) -> egui::Color32 {
        if temp > GPU_HIGH_TEMP_THRESHOLD {
            self.theme.danger
        } else if temp > GPU_MEDIUM_TEMP_THRESHOLD {
            self.theme.warning
        } else {
            self.theme.text_secondary
        }
    }

    #[inline]
    fn get_battery_color(&self, percentage: f32) -> egui::Color32 {
        if percentage <= BATTERY_LOW_THRESHOLD {
            self.theme.danger
        } else if percentage <= BATTERY_MEDIUM_THRESHOLD {
            self.theme.warning
        } else {
            self.theme.success
        }
    }
}

// ================= Utility Functions =================

#[inline]
fn format_bytes_to_gb(bytes: u64) -> String {
    let gb = bytes as f64 / BYTES_PER_GB;
    if gb >= 1000.0 {
        format!("{:.2} TB", gb / 1024.0)
    } else {
        format!("{:.1} GB", gb)
    }
}

#[inline]
fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

// ================= UI IMPLEMENTATION =================

impl eframe::App for SystemMonitor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.last_fast_refresh.elapsed() >= FAST_REFRESH_INTERVAL {
            self.refresh_fast_metrics();
            self.last_fast_refresh = Instant::now();
        }

        if self.last_slow_refresh.elapsed() >= SLOW_REFRESH_INTERVAL {
            self.refresh_slow_metrics();
            self.last_slow_refresh = Instant::now();
        }

        self.configure_style(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(self.theme.bg_primary))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.render_content(ui);
                    });
            });

        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

impl SystemMonitor {
    fn configure_style(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(16.0, 16.0);
        style.spacing.window_margin = egui::Margin::same(0.0);
        style.visuals.widgets.noninteractive.bg_fill = self.theme.bg_card;
        style.visuals.widgets.inactive.bg_fill = self.theme.bg_card;
        style.visuals.extreme_bg_color = self.theme.bg_secondary;
        style.visuals.window_shadow = egui::epaint::Shadow::NONE;
        ctx.set_style(style);
    }

    fn render_content(&mut self, ui: &mut egui::Ui) {
        let available_width = ui.available_width();
        let content_width = (available_width - (SIDE_PADDING * 2.0)).min(MAX_CONTENT_WIDTH);

        ui.allocate_ui_with_layout(
            egui::vec2(available_width, ui.available_height()),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.add_space(24.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(content_width, ui.available_height()),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        self.render_header(ui);
                        ui.add_space(24.0);

                        let metric_spacing = 16.0;
                        let metric_width = (content_width - metric_spacing) / 2.0;

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = metric_spacing;
                            let cpu_usage = self.avg_cpu_usage();
                            let mem_usage = self.memory_usage_percent();
                            self.render_metric_card(
                                ui,
                                metric_width,
                                "CPU",
                                cpu_usage,
                                self.get_usage_color(cpu_usage),
                            );
                            self.render_metric_card(
                                ui,
                                metric_width,
                                "Memory",
                                mem_usage,
                                self.get_usage_color(mem_usage),
                            );
                        });

                        ui.add_space(metric_spacing);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = metric_spacing;
                            let gpu_usage = self
                                .gpu_monitor
                                .get_discrete_gpus()
                                .first()
                                .map(|g| g.utilization)
                                .unwrap_or(
                                    self.gpu_monitor
                                        .get_integrated_gpus()
                                        .first()
                                        .map(|g| g.utilization)
                                        .unwrap_or(0.0),
                                );
                            self.render_metric_card(
                                ui,
                                metric_width,
                                "GPU",
                                gpu_usage,
                                self.get_usage_color(gpu_usage),
                            );
                            self.render_metric_card(
                                ui,
                                metric_width,
                                "Processes",
                                self.total_processes as f32,
                                self.theme.accent,
                            );
                        });

                        ui.add_space(24.0);

                        if self.battery_monitor.has_battery() {
                            self.render_battery_card(ui);
                            ui.add_space(24.0);
                        }

                        let col_spacing = 16.0;
                        let col_width = (content_width - (col_spacing * 2.0)) / 3.0;

                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = col_spacing;

                            ui.vertical(|ui| {
                                ui.set_width(col_width);
                                self.render_cpu_card(ui);
                            });

                            ui.vertical(|ui| {
                                ui.set_width(col_width);
                                self.render_memory_card(ui);
                                ui.add_space(16.0);

                                if self.gpu_monitor.has_integrated_gpu()
                                    || self.gpu_monitor.has_discrete_gpu()
                                {
                                    self.render_gpu_cards(ui);
                                }
                            });

                            ui.vertical(|ui| {
                                ui.set_width(col_width);
                                self.render_process_card(ui);
                                ui.add_space(16.0);

                                if self.npu_monitor.has_npu() {
                                    self.render_npu_card(ui);
                                    ui.add_space(16.0);
                                }

                                self.render_disk_card(ui);
                            });
                        });

                        ui.add_space(24.0);
                    },
                );
            },
        );
    }

    fn render_header(&mut self, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(self.theme.bg_card)
            .rounding(CARD_ROUNDING)
            .inner_margin(egui::vec2(28.0, 22.0))
            .stroke(egui::Stroke::new(1.5, self.theme.border))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("⚡")
                                    .size(32.0)
                                    .color(self.theme.accent),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("System Monitor")
                                    .size(30.0)
                                    .strong()
                                    .color(self.theme.text_primary),
                            );
                        });
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "Real-time performance monitoring with accurate metrics",
                            )
                            .size(13.0)
                            .color(self.theme.text_secondary),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.render_theme_selector(ui);
                    });
                });
            });
    }

    fn render_theme_selector(&mut self, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(self.theme.bg_elevated)
            .rounding(8.0)
            .inner_margin(egui::vec2(4.0, 4.0))
            .stroke(egui::Stroke::new(1.0, self.theme.border))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;

                    let themes = [
                        (ThemeMode::Dark, "🌙"),
                        (ThemeMode::Midnight, "🌃"),
                        (ThemeMode::Nord, "❄️"),
                        (ThemeMode::Light, "☀️"),
                    ];

                    for (mode, icon) in themes {
                        let is_selected = self.theme_mode == mode;
                        let button = egui::Button::new(egui::RichText::new(icon).size(16.0))
                            .fill(if is_selected {
                                self.theme.accent
                            } else {
                                self.theme.bg_card
                            })
                            .rounding(6.0)
                            .min_size(egui::vec2(36.0, 36.0));

                        if ui.add(button).clicked() && !is_selected {
                            self.theme_mode = mode;
                            self.theme = Theme::from_mode(mode);
                        }
                    }
                });
            });
    }

    fn render_metric_card(
        &self,
        ui: &mut egui::Ui,
        width: f32,
        label: &str,
        value: f32,
        color: egui::Color32,
    ) {
        egui::Frame::none()
            .fill(self.theme.bg_card)
            .rounding(CARD_ROUNDING)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, self.theme.border))
            .show(ui, |ui| {
                ui.set_width(width - 34.0);
                ui.set_min_height(85.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(label.to_uppercase())
                                .size(10.0)
                                .strong()
                                .color(self.theme.text_tertiary),
                        );

                        // Add status indicator
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if label != "Processes" {
                                let indicator_color = if value >= HIGH_USAGE_THRESHOLD {
                                    self.theme.danger
                                } else if value >= MEDIUM_USAGE_THRESHOLD {
                                    self.theme.warning
                                } else {
                                    self.theme.success
                                };
                                ui.painter().circle_filled(
                                    ui.cursor().min + egui::vec2(4.0, 4.0),
                                    3.0,
                                    indicator_color,
                                );
                                ui.add_space(8.0);
                            }
                        });
                    });
                    ui.add_space(8.0);

                    let display_text = if label == "Processes" {
                        format!("{:.0}", value)
                    } else {
                        format!("{:.1}%", value)
                    };

                    ui.label(
                        egui::RichText::new(display_text)
                            .size(28.0)
                            .strong()
                            .color(color),
                    );

                    if label != "Processes" {
                        ui.add_space(6.0);
                        let progress = egui::ProgressBar::new(value / 100.0)
                            .desired_height(5.0)
                            .fill(color)
                            .rounding(2.5);
                        ui.add(progress);
                    }
                });
            });
    }

    fn render_battery_card(&self, ui: &mut egui::Ui) {
        if let Some(battery) = &self.battery_monitor.battery_info {
            self.render_card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("BATTERY")
                                .size(10.0)
                                .strong()
                                .color(self.theme.text_tertiary),
                        );
                        ui.add_space(2.0);
                        let status = if battery.is_charging {
                            "⚡ Charging"
                        } else {
                            "🔋 Discharging"
                        };
                        ui.label(
                            egui::RichText::new(status)
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{:.0}%", battery.percentage))
                                .size(24.0)
                                .strong()
                                .color(self.get_battery_color(battery.percentage)),
                        );
                    });
                });

                ui.add_space(10.0);
                let progress = egui::ProgressBar::new(battery.percentage / 100.0)
                    .desired_height(6.0)
                    .fill(self.get_battery_color(battery.percentage))
                    .rounding(3.0);
                ui.add(progress);

                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;

                    if let (Some(current_wh), Some(full_wh)) =
                        (battery.capacity_wh, battery.energy_full_wh)
                    {
                        ui.label(
                            egui::RichText::new(format!(
                                "Capacity: {:.1} / {:.1} Wh",
                                current_wh, full_wh
                            ))
                            .size(12.0)
                            .color(self.theme.text_secondary),
                        );
                    }

                    if let Some(time) = battery.time_remaining {
                        let prefix = if battery.is_charging {
                            "Time to full:"
                        } else {
                            "Time remaining:"
                        };
                        ui.label(
                            egui::RichText::new(format!("{} {}", prefix, format_duration(time)))
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );
                    }

                    if let Some(power) = battery.power_consumption {
                        ui.label(
                            egui::RichText::new(format!("Power: {:.1}W", power))
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );
                    }

                    if let Some(health) = battery.health {
                        ui.label(
                            egui::RichText::new(format!("Health: {:.0}%", health))
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );
                    }
                });
            });
        }
    }

    fn render_cpu_card(&self, ui: &mut egui::Ui) {
        self.render_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("CPU")
                        .size(11.0)
                        .strong()
                        .color(self.theme.text_tertiary),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(temp) = self.cpu_package_temp {
                        let temp_color = self.get_temp_color(temp);
                        egui::Frame::none()
                            .fill(self.theme.bg_elevated)
                            .rounding(6.0)
                            .inner_margin(egui::vec2(8.0, 4.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format!("{}°C", temp as i32))
                                        .size(11.0)
                                        .strong()
                                        .color(temp_color),
                                );
                            });
                    }
                });
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(&self.cpu_info.brand)
                    .size(15.0)
                    .strong()
                    .color(self.theme.text_primary),
            );
            ui.add_space(2.0);

            let mut info_parts = vec![format!(
                "{} cores · {} threads",
                self.cpu_info.physical_cores, self.cpu_info.logical_cores
            )];

            if let (Some(base), Some(max)) =
                (self.cpu_info.base_frequency, self.cpu_info.max_frequency)
            {
                info_parts.push(format!("{:.2}-{:.2} GHz", base, max));
            }

            ui.label(
                egui::RichText::new(info_parts.join(" · "))
                    .size(11.0)
                    .color(self.theme.text_secondary),
            );

            ui.add_space(16.0);

            let avg_usage = self.avg_cpu_usage();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Average Usage")
                        .size(13.0)
                        .color(self.theme.text_secondary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{:.1}%", avg_usage))
                            .size(20.0)
                            .strong()
                            .color(self.get_usage_color(avg_usage)),
                    );
                });
            });
            ui.add_space(10.0);
            let progress = egui::ProgressBar::new(avg_usage / 100.0)
                .desired_height(7.0)
                .fill(self.get_usage_color(avg_usage))
                .rounding(3.5);
            ui.add(progress);

            ui.add_space(16.0);
            ui.add(egui::Separator::default().spacing(0.0).horizontal());
            ui.add_space(14.0);

            for (i, _history) in self.cpu_history.iter().enumerate() {
                let current = self.cpu_core_stats.get(i).map(|s| s.usage).unwrap_or(0.0);
                let core_stat = self.cpu_core_stats.get(i);

                egui::Frame::none()
                    .fill(self.theme.bg_elevated)
                    .rounding(INNER_CARD_ROUNDING)
                    .inner_margin(egui::vec2(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(
                                    egui::RichText::new(format!("Core {}", i))
                                        .size(11.0)
                                        .strong()
                                        .color(self.theme.text_primary),
                                );

                                if let Some(stat) = core_stat {
                                    let mut details = Vec::new();
                                    if let Some(freq) = stat.frequency {
                                        details.push(format!("{:.2}G", freq));
                                    }
                                    if let Some(temp) = stat.temperature {
                                        details.push(format!("{}°", temp as i32));
                                    }

                                    if !details.is_empty() {
                                        ui.label(
                                            egui::RichText::new(details.join(" · "))
                                                .size(10.0)
                                                .color(self.theme.text_tertiary),
                                        );
                                    }
                                }
                            });

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:.0}%", current))
                                            .size(12.0)
                                            .strong()
                                            .color(self.get_usage_color(current)),
                                    );

                                    ui.add_space(10.0);

                                    let available_width = ui.available_width() - 10.0;
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(available_width, 6.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            let progress = egui::ProgressBar::new(current / 100.0)
                                                .desired_width(available_width)
                                                .desired_height(6.0)
                                                .fill(self.get_usage_color(current))
                                                .rounding(3.0);
                                            ui.add(progress);
                                        },
                                    );
                                },
                            );
                        });
                    });

                if i < self.cpu_history.len() - 1 {
                    ui.add_space(6.0);
                }
            }
        });
    }

    fn render_memory_card(&self, ui: &mut egui::Ui) {
        let usage_pct = self.memory_usage_percent();

        self.render_card(ui, |ui| {
            ui.label(
                egui::RichText::new("MEMORY")
                    .size(10.0)
                    .strong()
                    .color(self.theme.text_tertiary),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{:.1}%", usage_pct))
                        .size(26.0)
                        .strong()
                        .color(self.get_usage_color(usage_pct)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{:.2} / {:.2} GB",
                            self.memory_used_gb(),
                            self.memory_total_gb()
                        ))
                        .size(12.0)
                        .strong()
                        .color(self.theme.text_secondary),
                    );
                });
            });

            ui.add_space(12.0);
            let progress = egui::ProgressBar::new(usage_pct / 100.0)
                .desired_height(7.0)
                .fill(self.get_usage_color(usage_pct))
                .rounding(3.5);
            ui.add(progress);

            ui.add_space(14.0);
            self.render_chart(
                ui,
                &self.memory_history,
                "memory_chart".to_string(),
                85.0,
                self.theme.accent,
            );
        });
    }

    fn render_process_card(&self, ui: &mut egui::Ui) {
        self.render_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("TOP PROCESSES")
                        .size(11.0)
                        .strong()
                        .color(self.theme.text_tertiary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Add a subtle badge showing process count
                    egui::Frame::none()
                        .fill(self.theme.bg_elevated)
                        .rounding(6.0)
                        .inner_margin(egui::vec2(8.0, 3.0))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("{} total", self.total_processes))
                                    .size(10.0)
                                    .color(self.theme.accent),
                            );
                        });
                });
            });

            ui.add_space(16.0);

            if self.top_processes.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(30.0);
                    ui.label(
                        egui::RichText::new("⏳")
                            .size(32.0)
                            .color(self.theme.text_tertiary),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Loading process data...")
                            .size(12.0)
                            .color(self.theme.text_tertiary),
                    );
                    ui.add_space(30.0);
                });
            } else {
                // Add header row
                egui::Frame::none()
                    .fill(self.theme.bg_elevated)
                    .rounding(egui::Rounding {
                        nw: INNER_CARD_ROUNDING,
                        ne: INNER_CARD_ROUNDING,
                        sw: 0.0,
                        se: 0.0,
                    })
                    .inner_margin(egui::vec2(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let name_width = ui.available_width() * 0.5;
                            ui.allocate_ui_with_layout(
                                egui::vec2(name_width, 14.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new("Process")
                                            .size(10.0)
                                            .strong()
                                            .color(self.theme.text_tertiary),
                                    );
                                },
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new("Memory")
                                            .size(10.0)
                                            .strong()
                                            .color(self.theme.text_tertiary),
                                    );
                                    ui.add_space(40.0);
                                    ui.label(
                                        egui::RichText::new("CPU")
                                            .size(10.0)
                                            .strong()
                                            .color(self.theme.text_tertiary),
                                    );
                                },
                            );
                        });
                    });

                egui::Frame::none()
                    .fill(self.theme.bg_elevated)
                    .rounding(egui::Rounding {
                        nw: 0.0,
                        ne: 0.0,
                        sw: INNER_CARD_ROUNDING,
                        se: INNER_CARD_ROUNDING,
                    })
                    .inner_margin(0.0)
                    .show(ui, |ui| {
                        for (i, process) in self.top_processes.iter().enumerate() {
                            // Alternating row colors for better readability
                            let row_bg = if i % 2 == 0 {
                                self.theme.bg_elevated
                            } else {
                                egui::Color32::from_rgba_premultiplied(
                                    self.theme.bg_elevated.r().saturating_sub(3),
                                    self.theme.bg_elevated.g().saturating_sub(3),
                                    self.theme.bg_elevated.b().saturating_sub(3),
                                    self.theme.bg_elevated.a(),
                                )
                            };

                            egui::Frame::none()
                                .fill(row_bg)
                                .inner_margin(egui::vec2(12.0, 10.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let name_width = ui.available_width() * 0.5;
                                        ui.add_sized(
                                            [name_width, 18.0],
                                            egui::Label::new(
                                                egui::RichText::new(&process.name)
                                                    .size(11.0)
                                                    .color(self.theme.text_primary),
                                            ),
                                        );

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "{:.0} MB",
                                                        process.memory_mb()
                                                    ))
                                                    .size(11.0)
                                                    .color(self.theme.text_secondary),
                                                );
                                                ui.add_space(16.0);

                                                // Add visual indicator for high CPU usage
                                                let cpu_color = if process.cpu_usage > 50.0 {
                                                    self.theme.danger
                                                } else if process.cpu_usage > 25.0 {
                                                    self.theme.warning
                                                } else {
                                                    self.theme.accent
                                                };

                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "{:.1}%",
                                                        process.cpu_usage
                                                    ))
                                                    .size(11.0)
                                                    .strong()
                                                    .color(cpu_color),
                                                );
                                            },
                                        );
                                    });
                                });

                            if i < self.top_processes.len() - 1 {
                                ui.add(egui::Separator::default().spacing(0.0).horizontal());
                            }
                        }
                    });
            }
        });
    }

    fn render_gpu_cards(&self, ui: &mut egui::Ui) {
        if self.gpu_monitor.has_discrete_gpu() {
            for (idx, gpu) in self.gpu_monitor.get_discrete_gpus().iter().enumerate() {
                let gpu_index = self
                    .gpu_monitor
                    .gpus
                    .iter()
                    .position(|g| g.name == gpu.name && !g.is_integrated)
                    .unwrap_or(idx);
                self.render_gpu_card(ui, gpu_index, gpu, "DISCRETE GPU");
                ui.add_space(16.0);
            }
        }

        if self.gpu_monitor.has_integrated_gpu() {
            for (idx, gpu) in self.gpu_monitor.get_integrated_gpus().iter().enumerate() {
                let gpu_index = self
                    .gpu_monitor
                    .gpus
                    .iter()
                    .position(|g| g.name == gpu.name && g.is_integrated)
                    .unwrap_or(idx);
                self.render_gpu_card(ui, gpu_index, gpu, "INTEGRATED GPU");
                if idx < self.gpu_monitor.get_integrated_gpus().len() - 1 {
                    ui.add_space(16.0);
                }
            }
        }
    }

    fn render_gpu_card(&self, ui: &mut egui::Ui, index: usize, gpu: &GpuInfo, label: &str) {
        self.render_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(label)
                        .size(11.0)
                        .strong()
                        .color(self.theme.text_tertiary),
                );

                // Show type badge
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let badge_text = if gpu.is_integrated { "iGPU" } else { "dGPU" };
                    let badge_color = if gpu.is_integrated {
                        self.theme.text_tertiary
                    } else {
                        self.theme.accent
                    };

                    egui::Frame::none()
                        .fill(self.theme.bg_elevated)
                        .rounding(4.0)
                        .inner_margin(egui::vec2(6.0, 2.0))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(badge_text).size(9.0).color(badge_color));
                        });
                });
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(&gpu.name)
                    .size(14.0)
                    .strong()
                    .color(self.theme.text_primary),
            );

            ui.add_space(16.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("GPU Utilization")
                        .size(12.0)
                        .color(self.theme.text_secondary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{:.1}%", gpu.utilization))
                            .size(18.0)
                            .strong()
                            .color(self.get_usage_color(gpu.utilization)),
                    );
                });
            });
            ui.add_space(10.0);
            let progress = egui::ProgressBar::new(gpu.utilization / 100.0)
                .desired_height(7.0)
                .fill(self.get_usage_color(gpu.utilization))
                .rounding(3.5);
            ui.add(progress);

            // Show VRAM/Shared Memory for all GPUs
            if gpu.memory_total > 0 {
                ui.add_space(16.0);
                let vram_pct = gpu.memory_usage_percent();
                let memory_label = if gpu.is_integrated {
                    "Shared Memory"
                } else {
                    "VRAM"
                };

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(memory_label)
                            .size(12.0)
                            .color(self.theme.text_secondary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{:.2} / {:.2} GB ({:.1}%)",
                                gpu.memory_used_gb(),
                                gpu.memory_total_gb(),
                                vram_pct
                            ))
                            .size(11.0)
                            .strong()
                            .color(self.theme.text_primary),
                        );
                    });
                });
                ui.add_space(8.0);
                let progress = egui::ProgressBar::new(vram_pct / 100.0)
                    .desired_height(6.0)
                    .fill(self.get_usage_color(vram_pct))
                    .rounding(3.0);
                ui.add(progress);
            }

            ui.add_space(14.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                if let Some(temp) = gpu.temperature {
                    ui.label(
                        egui::RichText::new(format!("🌡 {:.0}°C", temp))
                            .size(11.0)
                            .color(self.get_temp_color(temp)),
                    );
                }
                if let Some(power) = gpu.power_usage {
                    ui.label(
                        egui::RichText::new(format!("⚡ {:.1}W", power))
                            .size(11.0)
                            .color(self.theme.text_secondary),
                    );
                }
            });

            if let Some(history) = self.gpu_monitor.gpu_history.get(index) {
                if !history.is_empty() && history.len() > 1 {
                    ui.add_space(14.0);
                    self.render_chart(
                        ui,
                        history,
                        format!("gpu_{}", index),
                        85.0,
                        self.theme.accent,
                    );
                }
            }
        });
    }

    fn render_npu_card(&self, ui: &mut egui::Ui) {
        if let Some(npu) = &self.npu_monitor.npu_info {
            self.render_card(ui, |ui| {
                ui.label(
                    egui::RichText::new("NPU (AI ACCELERATOR)")
                        .size(10.0)
                        .strong()
                        .color(self.theme.text_tertiary),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(&npu.name)
                        .size(13.0)
                        .strong()
                        .color(self.theme.text_primary),
                );

                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("NPU Utilization")
                            .size(11.0)
                            .color(self.theme.text_secondary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{:.0}%", npu.utilization))
                                .size(15.0)
                                .strong()
                                .color(self.get_usage_color(npu.utilization)),
                        );
                    });
                });
                ui.add_space(6.0);
                let progress = egui::ProgressBar::new(npu.utilization / 100.0)
                    .desired_height(5.0)
                    .fill(self.get_usage_color(npu.utilization))
                    .rounding(2.5);
                ui.add(progress);

                ui.add_space(12.0);

                if !npu.active_models.is_empty() {
                    ui.label(
                        egui::RichText::new("Active AI Workloads")
                            .size(10.0)
                            .color(self.theme.text_tertiary),
                    );
                    ui.add_space(4.0);

                    egui::Frame::none()
                        .fill(self.theme.bg_elevated)
                        .rounding(INNER_CARD_ROUNDING)
                        .inner_margin(egui::vec2(8.0, 6.0))
                        .show(ui, |ui| {
                            for (i, model) in npu.active_models.iter().take(5).enumerate() {
                                ui.label(
                                    egui::RichText::new(format!("• {}", model))
                                        .size(10.0)
                                        .color(self.theme.text_primary),
                                );
                                if i < npu.active_models.len().min(5) - 1 {
                                    ui.add_space(3.0);
                                }
                            }
                            if npu.active_models.len() > 5 {
                                ui.add_space(3.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "... and {} more",
                                        npu.active_models.len() - 5
                                    ))
                                    .size(9.0)
                                    .color(self.theme.text_tertiary),
                                );
                            }
                        });

                    ui.add_space(12.0);
                }

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;

                    if let Some(freq) = npu.frequency {
                        ui.label(
                            egui::RichText::new(format!("Freq: {:.0} MHz", freq))
                                .size(11.0)
                                .color(self.theme.text_secondary),
                        );
                    }

                    if let Some(temp) = npu.temperature {
                        ui.label(
                            egui::RichText::new(format!("Temp: {:.0}°C", temp))
                                .size(11.0)
                                .color(self.get_temp_color(temp)),
                        );
                    }

                    if let Some(power) = npu.power_usage {
                        ui.label(
                            egui::RichText::new(format!("Power: {:.1}W", power))
                                .size(11.0)
                                .color(self.theme.text_secondary),
                        );
                    }
                });

                if !self.npu_monitor.npu_history.is_empty()
                    && self.npu_monitor.npu_history.len() > 1
                {
                    ui.add_space(12.0);
                    self.render_chart(
                        ui,
                        &self.npu_monitor.npu_history,
                        "npu_chart".to_string(),
                        70.0,
                        self.theme.accent,
                    );
                }
            });
        }
    }

    fn render_disk_card(&self, ui: &mut egui::Ui) {
        if self.disk_stats.is_empty() {
            return;
        }

        self.render_card(ui, |ui| {
            ui.label(
                egui::RichText::new("STORAGE")
                    .size(10.0)
                    .strong()
                    .color(self.theme.text_tertiary),
            );
            ui.add_space(14.0);

            for (i, disk) in self.disk_stats.iter().enumerate() {
                egui::Frame::none()
                    .fill(self.theme.bg_elevated)
                    .rounding(INNER_CARD_ROUNDING)
                    .inner_margin(egui::vec2(12.0, 12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let icon = if disk.mount_point.contains("home")
                                        || disk.mount_point == "/"
                                    {
                                        "🏠"
                                    } else {
                                        "💾"
                                    };
                                    ui.label(egui::RichText::new(icon).size(12.0));
                                    ui.label(
                                        egui::RichText::new(&disk.mount_point)
                                            .size(12.0)
                                            .strong()
                                            .color(self.theme.text_primary),
                                    );
                                });
                                ui.add_space(3.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} / {} ({:.1}% used)",
                                        format_bytes_to_gb(disk.used_space()),
                                        format_bytes_to_gb(disk.total_space),
                                        disk.usage_percent
                                    ))
                                    .size(10.0)
                                    .color(self.theme.text_secondary),
                                );
                            });

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:.1}%", disk.usage_percent))
                                            .size(13.0)
                                            .strong()
                                            .color(self.get_usage_color(disk.usage_percent)),
                                    );
                                },
                            );
                        });

                        ui.add_space(10.0);
                        let progress = egui::ProgressBar::new(disk.usage_percent / 100.0)
                            .desired_height(5.0)
                            .fill(self.get_usage_color(disk.usage_percent))
                            .rounding(2.5);
                        ui.add(progress);
                    });

                if i < self.disk_stats.len() - 1 {
                    ui.add_space(12.0);
                }
            }
        });
    }

    fn render_chart(
        &self,
        ui: &mut egui::Ui,
        history: &VecDeque<f32>,
        id: String,
        height: f32,
        color: egui::Color32,
    ) {
        let points: PlotPoints = history
            .iter()
            .enumerate()
            .map(|(x, y)| [x as f64, *y as f64])
            .collect();

        egui::Frame::none()
            .fill(self.theme.bg_elevated)
            .rounding(INNER_CARD_ROUNDING)
            .inner_margin(6.0)
            .show(ui, |ui| {
                Plot::new(id)
                    .height(height)
                    .width(ui.available_width())
                    .allow_scroll(false)
                    .allow_zoom(false)
                    .allow_drag(false)
                    .allow_boxed_zoom(false)
                    .show_axes([false, false])
                    .show_background(false)
                    .show(ui, |plot_ui| {
                        plot_ui.line(Line::new(points).width(2.0).color(color));
                    });
            });
    }

    fn render_card(&self, ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::none()
            .fill(self.theme.bg_card)
            .rounding(CARD_ROUNDING)
            .inner_margin(20.0)
            .stroke(egui::Stroke::new(1.0, self.theme.border))
            .show(ui, add_contents);
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 1000.0])
            .with_min_inner_size([800.0, 800.0])
            .with_title("System Monitor"),
        ..Default::default()
    };

    eframe::run_native(
        "System Monitor",
        options,
        Box::new(|_cc| Ok(Box::new(SystemMonitor::new()))),
    )
}
