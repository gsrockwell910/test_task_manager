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
const FAST_REFRESH_INTERVAL: Duration = Duration::from_millis(200);
const SLOW_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const INITIAL_CPU_SAMPLE_DELAY: Duration = Duration::from_millis(200);

const CARD_ROUNDING: f32 = 12.0;
const INNER_CARD_ROUNDING: f32 = 8.0;
const CARD_PADDING: f32 = 24.0;
const MAX_CONTENT_WIDTH: f32 = 1600.0;
const SIDE_PADDING: f32 = 40.0;

const TOP_PROCESS_COUNT: usize = 10;
const BYTES_PER_MB: f64 = 1024.0 * 1024.0;
const BYTES_PER_GB: f64 = 1024.0 * 1024.0 * 1024.0;

const DESKTOP_BREAKPOINT: f32 = 1200.0;
const TABLET_BREAKPOINT: f32 = 768.0;

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
    // accent_hover: egui::Color32,
    text_primary: egui::Color32,
    text_secondary: egui::Color32,
    text_tertiary: egui::Color32,
    success: egui::Color32,
    warning: egui::Color32,
    danger: egui::Color32,
    // divider: egui::Color32,
    border: egui::Color32,
    // glow: egui::Color32,
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
            bg_primary: egui::Color32::from_rgb(15, 17, 24),
            bg_secondary: egui::Color32::from_rgb(20, 23, 31),
            bg_card: egui::Color32::from_rgb(24, 27, 36),
            bg_elevated: egui::Color32::from_rgb(30, 34, 44),
            accent: egui::Color32::from_rgb(99, 102, 241),
            // accent_hover: egui::Color32::from_rgb(129, 132, 251),
            text_primary: egui::Color32::from_rgb(240, 242, 245),
            text_secondary: egui::Color32::from_rgb(156, 163, 175),
            text_tertiary: egui::Color32::from_rgb(107, 114, 128),
            success: egui::Color32::from_rgb(34, 197, 94),
            warning: egui::Color32::from_rgb(251, 191, 36),
            danger: egui::Color32::from_rgb(239, 68, 68),
            // divider: egui::Color32::from_rgb(55, 60, 72),
            border: egui::Color32::from_rgb(42, 47, 60),
            // glow: egui::Color32::from_rgba_premultiplied(99, 102, 241, 40),
        }
    }

    const fn midnight() -> Self {
        Self {
            bg_primary: egui::Color32::from_rgb(10, 12, 16),
            bg_secondary: egui::Color32::from_rgb(16, 18, 24),
            bg_card: egui::Color32::from_rgb(20, 22, 28),
            bg_elevated: egui::Color32::from_rgb(26, 28, 36),
            accent: egui::Color32::from_rgb(56, 189, 248),
            // accent_hover: egui::Color32::from_rgb(96, 209, 255),
            text_primary: egui::Color32::from_rgb(248, 250, 252),
            text_secondary: egui::Color32::from_rgb(148, 163, 184),
            text_tertiary: egui::Color32::from_rgb(100, 116, 139),
            success: egui::Color32::from_rgb(16, 185, 129),
            warning: egui::Color32::from_rgb(245, 158, 11),
            danger: egui::Color32::from_rgb(248, 113, 113),
            // divider: egui::Color32::from_rgb(45, 50, 62),
            border: egui::Color32::from_rgb(38, 42, 54),
            // glow: egui::Color32::from_rgba_premultiplied(56, 189, 248, 35),
        }
    }

    const fn nord() -> Self {
        Self {
            bg_primary: egui::Color32::from_rgb(46, 52, 64),
            bg_secondary: egui::Color32::from_rgb(59, 66, 82),
            bg_card: egui::Color32::from_rgb(67, 76, 94),
            bg_elevated: egui::Color32::from_rgb(76, 86, 106),
            accent: egui::Color32::from_rgb(136, 192, 208),
            // accent_hover: egui::Color32::from_rgb(156, 212, 228),
            text_primary: egui::Color32::from_rgb(236, 239, 244),
            text_secondary: egui::Color32::from_rgb(216, 222, 233),
            text_tertiary: egui::Color32::from_rgb(143, 157, 177),
            success: egui::Color32::from_rgb(163, 190, 140),
            warning: egui::Color32::from_rgb(235, 203, 139),
            danger: egui::Color32::from_rgb(191, 97, 106),
            // divider: egui::Color32::from_rgb(94, 105, 126),
            border: egui::Color32::from_rgb(81, 91, 112),
            // glow: egui::Color32::from_rgba_premultiplied(136, 192, 208, 30),
        }
    }

    const fn light() -> Self {
        Self {
            bg_primary: egui::Color32::from_rgb(250, 250, 252),
            bg_secondary: egui::Color32::from_rgb(241, 243, 247),
            bg_card: egui::Color32::from_rgb(255, 255, 255),
            bg_elevated: egui::Color32::from_rgb(248, 249, 251),
            accent: egui::Color32::from_rgb(79, 70, 229),
            // accent_hover: egui::Color32::from_rgb(99, 90, 249),
            text_primary: egui::Color32::from_rgb(17, 24, 39),
            text_secondary: egui::Color32::from_rgb(75, 85, 99),
            text_tertiary: egui::Color32::from_rgb(156, 163, 175),
            success: egui::Color32::from_rgb(16, 185, 129),
            warning: egui::Color32::from_rgb(245, 158, 11),
            danger: egui::Color32::from_rgb(239, 68, 68),
            // divider: egui::Color32::from_rgb(229, 231, 235),
            border: egui::Color32::from_rgb(229, 231, 235),
            // glow: egui::Color32::from_rgba_premultiplied(79, 70, 229, 20),
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
    // file_system: String,
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
    // smt_enabled: bool,
    brand: String,
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
}

// ================= GPU Monitor =================

struct GpuMonitor {
    gpus: Vec<GpuInfo>,
    gpu_history: Vec<VecDeque<f32>>,
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
        self.refresh_amd_gpu_linux();

        if self.gpu_history.len() != self.gpus.len() {
            self.gpu_history
                .resize(self.gpus.len(), VecDeque::with_capacity(HISTORY_SIZE));
        }

        for (i, gpu) in self.gpus.iter().enumerate() {
            if let Some(history) = self.gpu_history.get_mut(i) {
                if history.len() >= HISTORY_SIZE {
                    history.pop_front();
                }
                history.push_back(gpu.utilization);
            }
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
                    let vendor = vendor.trim();
                    if vendor != "0x1002" {
                        continue;
                    }
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
                    busy_str.trim().parse::<f32>().unwrap_or(0.0)
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
                                temperature = Some(temp_millidegrees / 1000.0);
                                break;
                            }
                        }
                    }
                }

                let power_path = device_path.join("hwmon");
                let mut power_usage = None;
                if let Ok(hwmon_entries) = fs::read_dir(&power_path) {
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
                    || name_lower.contains("iris")
                    || (name_lower.contains("hd graphics") && name_lower.contains("intel"))
                    || (name_lower.contains("radeon")
                        && name_lower.contains("graphics")
                        && !name_lower.contains("rx"));

                let (memory_used, memory_total) = device
                    .memory_info()
                    .map(|m| (m.used, m.total))
                    .unwrap_or((0, 1));
                let memory_gb = memory_total as f64 / BYTES_PER_GB;
                let is_integrated = is_integrated_by_name || memory_gb < 2.0;

                let utilization = device
                    .utilization_rates()
                    .ok()
                    .map(|u| u.gpu as f32)
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

                    self.battery_info = Some(BatteryInfo {
                        percentage,
                        is_charging,
                        time_remaining,
                        power_consumption,
                        health,
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

// ================= Layout Configuration =================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutMode {
    Desktop,
    Tablet,
    Mobile,
}

impl LayoutMode {
    fn from_width(width: f32) -> Self {
        if width >= DESKTOP_BREAKPOINT {
            Self::Desktop
        } else if width >= TABLET_BREAKPOINT {
            Self::Tablet
        } else {
            Self::Mobile
        }
    }

    fn stats_columns(self) -> usize {
        match self {
            Self::Desktop => 4,
            Self::Tablet => 2,
            Self::Mobile => 1,
        }
    }

    fn content_columns(self) -> usize {
        match self {
            Self::Desktop => 2,
            Self::Tablet => 1,
            Self::Mobile => 1,
        }
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
    gpu_monitor: GpuMonitor,
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
        // let smt_enabled = cpu_count > physical_cores;

        let cpu_brand = system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().to_string())
            .unwrap_or_else(|| "Unknown CPU".to_string());

        let cpu_info = CpuInfo {
            physical_cores,
            logical_cores: cpu_count,
            // smt_enabled,
            brand: cpu_brand,
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
            gpu_monitor: GpuMonitor::new(),
            battery_monitor: BatteryMonitor::new(),
            last_fast_refresh: Instant::now(),
            last_slow_refresh: Instant::now(),
            theme_mode,
            theme: Theme::from_mode(theme_mode),
        }
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
        }

        let mem_usage = self.memory_usage_percent();
        if self.memory_history.len() >= HISTORY_SIZE {
            self.memory_history.pop_front();
        }
        self.memory_history.push_back(mem_usage);
    }

    fn refresh_slow_metrics(&mut self) {
        self.refresh_process_stats();
        self.refresh_disk_stats();
        self.gpu_monitor.refresh();
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
                    // file_system: disk.file_system().to_string_lossy().to_string(),
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
    format!("{:.1} GB", bytes as f64 / BYTES_PER_GB)
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

        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

impl SystemMonitor {
    fn configure_style(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(20.0, 20.0);
        style.visuals.widgets.noninteractive.bg_fill = self.theme.bg_card;
        style.visuals.widgets.inactive.bg_fill = self.theme.bg_card;
        style.visuals.extreme_bg_color = self.theme.bg_secondary;
        style.visuals.window_shadow.color = egui::Color32::from_black_alpha(60);
        ctx.set_style(style);
    }

    fn render_content(&mut self, ui: &mut egui::Ui) {
        let available_width = ui.available_width();
        let content_width = (available_width - (SIDE_PADDING * 2.0)).min(MAX_CONTENT_WIDTH);
        let layout_mode = LayoutMode::from_width(content_width);

        ui.allocate_ui_with_layout(
            egui::vec2(available_width, ui.available_height()),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.add_space(40.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(content_width, ui.available_height()),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        self.render_header(ui);
                        ui.add_space(32.0);
                        self.render_stats_grid(ui, layout_mode);
                        ui.add_space(24.0);

                        if self.battery_monitor.has_battery() {
                            self.render_battery_card(ui);
                            ui.add_space(24.0);
                        }

                        let columns = layout_mode.content_columns();
                        if columns == 2 {
                            self.render_two_column_layout(ui, layout_mode);
                        } else {
                            self.render_single_column_layout(ui);
                        }

                        ui.add_space(40.0);
                    },
                );
            },
        );
    }

    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("System Monitor")
                        .size(32.0)
                        .strong()
                        .color(self.theme.text_primary),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Real-time system performance metrics")
                        .size(14.0)
                        .color(self.theme.text_tertiary),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_theme_selector(ui);
            });
        });
    }

    fn render_theme_selector(&mut self, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(self.theme.bg_card)
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
                                self.theme.bg_elevated
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

    fn render_stats_grid(&self, ui: &mut egui::Ui, layout: LayoutMode) {
        let columns = layout.stats_columns();
        let spacing = 16.0;
        let available = ui.available_width();
        let total_spacing = spacing * (columns - 1) as f32;
        let card_width = (available - total_spacing) / columns as f32;

        let cpu_usage = self.avg_cpu_usage();
        let mem_usage = self.memory_usage_percent();
        let gpu_usage = self
            .gpu_monitor
            .get_discrete_gpus()
            .first()
            .map(|g| g.utilization)
            .unwrap_or(0.0);

        match columns {
            1 => {
                self.render_metric_card(
                    ui,
                    available,
                    "CPU",
                    cpu_usage,
                    self.get_usage_color(cpu_usage),
                );
                ui.add_space(spacing);
                self.render_metric_card(
                    ui,
                    available,
                    "Memory",
                    mem_usage,
                    self.get_usage_color(mem_usage),
                );
                ui.add_space(spacing);
                self.render_metric_card(
                    ui,
                    available,
                    "Processes",
                    self.total_processes as f32,
                    self.theme.accent,
                );
                if self.gpu_monitor.has_discrete_gpu() {
                    ui.add_space(spacing);
                    self.render_metric_card(
                        ui,
                        available,
                        "GPU",
                        gpu_usage,
                        self.get_usage_color(gpu_usage),
                    );
                }
            }
            2 => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = spacing;
                    self.render_metric_card(
                        ui,
                        card_width,
                        "CPU",
                        cpu_usage,
                        self.get_usage_color(cpu_usage),
                    );
                    self.render_metric_card(
                        ui,
                        card_width,
                        "Memory",
                        mem_usage,
                        self.get_usage_color(mem_usage),
                    );
                });
                ui.add_space(spacing);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = spacing;
                    self.render_metric_card(
                        ui,
                        card_width,
                        "Processes",
                        self.total_processes as f32,
                        self.theme.accent,
                    );
                    if self.gpu_monitor.has_discrete_gpu() {
                        self.render_metric_card(
                            ui,
                            card_width,
                            "GPU",
                            gpu_usage,
                            self.get_usage_color(gpu_usage),
                        );
                    }
                });
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = spacing;
                    self.render_metric_card(
                        ui,
                        card_width,
                        "CPU",
                        cpu_usage,
                        self.get_usage_color(cpu_usage),
                    );
                    self.render_metric_card(
                        ui,
                        card_width,
                        "Memory",
                        mem_usage,
                        self.get_usage_color(mem_usage),
                    );
                    self.render_metric_card(
                        ui,
                        card_width,
                        "Processes",
                        self.total_processes as f32,
                        self.theme.accent,
                    );
                    if self.gpu_monitor.has_discrete_gpu() {
                        self.render_metric_card(
                            ui,
                            card_width,
                            "GPU",
                            gpu_usage,
                            self.get_usage_color(gpu_usage),
                        );
                    }
                });
            }
        }
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
            .inner_margin(20.0)
            .stroke(egui::Stroke::new(1.0, self.theme.border))
            .shadow(egui::epaint::Shadow {
                offset: egui::vec2(0.0, 2.0),
                blur: 8.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(30),
            })
            .show(ui, |ui| {
                ui.set_width(width - 42.0);
                ui.set_min_height(110.0);
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(label.to_uppercase())
                            .size(10.0)
                            .strong()
                            .color(self.theme.text_tertiary),
                    );
                    ui.add_space(12.0);

                    let display_text = if label == "Processes" {
                        format!("{:.0}", value)
                    } else {
                        format!("{:.1}%", value)
                    };

                    ui.label(
                        egui::RichText::new(display_text)
                            .size(36.0)
                            .strong()
                            .color(color),
                    );

                    if label != "Processes" {
                        ui.add_space(8.0);
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
                        ui.add_space(6.0);
                        let status = if battery.is_charging {
                            "⚡ Charging"
                        } else {
                            "🔋 Discharging"
                        };
                        ui.label(
                            egui::RichText::new(status)
                                .size(13.0)
                                .color(self.theme.text_secondary),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{:.0}%", battery.percentage))
                                .size(32.0)
                                .strong()
                                .color(self.get_battery_color(battery.percentage)),
                        );
                    });
                });

                ui.add_space(16.0);
                let progress = egui::ProgressBar::new(battery.percentage / 100.0)
                    .desired_height(8.0)
                    .fill(self.get_battery_color(battery.percentage))
                    .rounding(4.0);
                ui.add(progress);

                ui.add_space(16.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 20.0;
                    if let Some(time) = battery.time_remaining {
                        let prefix = if battery.is_charging {
                            "⏱️ Full in"
                        } else {
                            "⏱️ Remaining"
                        };
                        ui.label(
                            egui::RichText::new(format!("{}: {}", prefix, format_duration(time)))
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );
                    }
                    if let Some(power) = battery.power_consumption {
                        ui.label(
                            egui::RichText::new(format!("⚡ {:.1}W", power))
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );
                    }
                    if let Some(health) = battery.health {
                        ui.label(
                            egui::RichText::new(format!("❤️ Health: {:.0}%", health))
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );
                    }
                });
            });
        }
    }

    fn render_two_column_layout(&mut self, ui: &mut egui::Ui, _layout: LayoutMode) {
        let spacing = 20.0;
        let available = ui.available_width();
        let col_width = (available - spacing) / 2.0;

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = spacing;

            ui.vertical(|ui| {
                ui.set_width(col_width);
                self.render_cpu_card(ui);
                ui.add_space(24.0);
                self.render_memory_card(ui);
                ui.add_space(24.0);
                if self.gpu_monitor.has_integrated_gpu() || self.gpu_monitor.has_discrete_gpu() {
                    self.render_gpu_cards(ui);
                    ui.add_space(24.0);
                }
            });

            ui.vertical(|ui| {
                ui.set_width(col_width);
                self.render_process_card(ui);
                ui.add_space(24.0);
                self.render_disk_card(ui);
            });
        });
    }

    fn render_single_column_layout(&mut self, ui: &mut egui::Ui) {
        self.render_cpu_card(ui);
        ui.add_space(24.0);
        self.render_memory_card(ui);
        ui.add_space(24.0);
        self.render_process_card(ui);
        ui.add_space(24.0);
        if self.gpu_monitor.has_integrated_gpu() || self.gpu_monitor.has_discrete_gpu() {
            self.render_gpu_cards(ui);
            ui.add_space(24.0);
        }
        self.render_disk_card(ui);
    }

    fn render_cpu_card(&self, ui: &mut egui::Ui) {
        self.render_card(ui, |ui| {
            ui.label(
                egui::RichText::new("CPU")
                    .size(10.0)
                    .strong()
                    .color(self.theme.text_tertiary),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(&self.cpu_info.brand)
                    .size(16.0)
                    .strong()
                    .color(self.theme.text_primary),
            );
            ui.add_space(3.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} cores · {} threads",
                    self.cpu_info.physical_cores, self.cpu_info.logical_cores
                ))
                .size(12.0)
                .color(self.theme.text_secondary),
            );

            ui.add_space(20.0);

            let avg_usage = self.avg_cpu_usage();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Average Usage")
                        .size(12.0)
                        .color(self.theme.text_secondary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{:.1}%", avg_usage))
                            .size(18.0)
                            .strong()
                            .color(self.get_usage_color(avg_usage)),
                    );
                });
            });
            ui.add_space(10.0);
            let progress = egui::ProgressBar::new(avg_usage / 100.0)
                .desired_height(8.0)
                .fill(self.get_usage_color(avg_usage))
                .rounding(4.0);
            ui.add(progress);

            ui.add_space(20.0);
            ui.add(egui::Separator::default().spacing(0.0).horizontal());
            ui.add_space(20.0);

            for (i, history) in self.cpu_history.iter().enumerate() {
                let current = history.back().copied().unwrap_or(0.0);

                egui::Frame::none()
                    .fill(self.theme.bg_elevated)
                    .rounding(INNER_CARD_ROUNDING)
                    .inner_margin(egui::vec2(14.0, 12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("Core {}", i))
                                    .size(12.0)
                                    .color(self.theme.text_secondary),
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:.0}%", current))
                                            .size(12.0)
                                            .strong()
                                            .color(self.theme.text_primary),
                                    );

                                    ui.add_space(12.0);

                                    let available_width = ui.available_width() - 12.0;
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(available_width, 8.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            let progress = egui::ProgressBar::new(current / 100.0)
                                                .desired_width(available_width)
                                                .desired_height(8.0)
                                                .fill(self.get_usage_color(current))
                                                .rounding(4.0);
                                            ui.add(progress);
                                        },
                                    );
                                },
                            );
                        });
                    });

                if i < self.cpu_history.len() - 1 {
                    ui.add_space(10.0);
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
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{:.1}%", usage_pct))
                        .size(32.0)
                        .strong()
                        .color(self.get_usage_color(usage_pct)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{:.1} / {:.1} GB",
                            self.memory_used_gb(),
                            self.memory_total_gb()
                        ))
                        .size(13.0)
                        .color(self.theme.text_secondary),
                    );
                });
            });

            ui.add_space(14.0);
            let progress = egui::ProgressBar::new(usage_pct / 100.0)
                .desired_height(8.0)
                .fill(self.get_usage_color(usage_pct))
                .rounding(4.0);
            ui.add(progress);

            ui.add_space(20.0);
            self.render_chart(
                ui,
                &self.memory_history,
                "memory_chart".to_string(),
                120.0,
                self.theme.accent,
            );
        });
    }

    fn render_process_card(&self, ui: &mut egui::Ui) {
        self.render_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("TOP PROCESSES")
                        .size(10.0)
                        .strong()
                        .color(self.theme.text_tertiary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} total", self.total_processes))
                            .size(10.0)
                            .color(self.theme.text_tertiary),
                    );
                });
            });

            ui.add_space(18.0);

            if self.top_processes.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(
                        egui::RichText::new("No process data available")
                            .size(13.0)
                            .color(self.theme.text_tertiary),
                    );
                    ui.add_space(40.0);
                });
            } else {
                egui::Frame::none()
                    .fill(self.theme.bg_elevated)
                    .rounding(INNER_CARD_ROUNDING)
                    .inner_margin(1.0)
                    .show(ui, |ui| {
                        for (i, process) in self.top_processes.iter().enumerate() {
                            egui::Frame::none()
                                .inner_margin(egui::vec2(14.0, 12.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let name_width = ui.available_width() * 0.5;
                                        ui.add_sized(
                                            [name_width, 20.0],
                                            egui::Label::new(
                                                egui::RichText::new(&process.name)
                                                    .size(12.0)
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
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "{:.1}%",
                                                        process.cpu_usage
                                                    ))
                                                    .size(11.0)
                                                    .strong()
                                                    .color(self.theme.accent),
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
                ui.add_space(24.0);
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
                    ui.add_space(24.0);
                }
            }
        }
    }

    fn render_gpu_card(&self, ui: &mut egui::Ui, index: usize, gpu: &GpuInfo, label: &str) {
        self.render_card(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .size(10.0)
                    .strong()
                    .color(self.theme.text_tertiary),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(&gpu.name)
                    .size(16.0)
                    .strong()
                    .color(self.theme.text_primary),
            );

            ui.add_space(18.0);

            if gpu.utilization > 0.0 || gpu.memory_total > 0 {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("GPU Utilization")
                            .size(12.0)
                            .color(self.theme.text_secondary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{:.0}%", gpu.utilization))
                                .size(18.0)
                                .strong()
                                .color(self.get_usage_color(gpu.utilization)),
                        );
                    });
                });
                ui.add_space(10.0);
                let progress = egui::ProgressBar::new(gpu.utilization / 100.0)
                    .desired_height(8.0)
                    .fill(self.get_usage_color(gpu.utilization))
                    .rounding(4.0);
                ui.add(progress);

                if gpu.memory_total > 0 {
                    ui.add_space(18.0);
                    let vram_pct = gpu.memory_usage_percent();
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("VRAM Usage")
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{:.1} / {:.1} GB",
                                    gpu.memory_used_gb(),
                                    gpu.memory_total_gb()
                                ))
                                .size(12.0)
                                .color(self.theme.text_primary),
                            );
                        });
                    });
                    ui.add_space(10.0);
                    let progress = egui::ProgressBar::new(vram_pct / 100.0)
                        .desired_height(8.0)
                        .fill(self.get_usage_color(vram_pct))
                        .rounding(4.0);
                    ui.add(progress);
                }

                ui.add_space(18.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 20.0;
                    if let Some(temp) = gpu.temperature {
                        ui.label(
                            egui::RichText::new(format!("🌡️ {:.0}°C", temp))
                                .size(12.0)
                                .color(self.get_temp_color(temp)),
                        );
                    }
                    if let Some(power) = gpu.power_usage {
                        ui.label(
                            egui::RichText::new(format!("⚡ {:.0}W", power))
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );
                    }
                });

                if let Some(history) = self.gpu_monitor.gpu_history.get(index) {
                    if !history.is_empty() && history.len() > 1 {
                        ui.add_space(18.0);
                        self.render_chart(
                            ui,
                            history,
                            format!("gpu_{}", index),
                            100.0,
                            self.theme.accent,
                        );
                    }
                }
            } else {
                ui.label(
                    egui::RichText::new("⚠️ Limited monitoring data available")
                        .size(12.0)
                        .color(self.theme.warning),
                );
            }
        });
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
            ui.add_space(18.0);

            for (i, disk) in self.disk_stats.iter().enumerate() {
                egui::Frame::none()
                    .fill(self.theme.bg_elevated)
                    .rounding(INNER_CARD_ROUNDING)
                    .inner_margin(egui::vec2(14.0, 14.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(&disk.mount_point)
                                        .size(13.0)
                                        .strong()
                                        .color(self.theme.text_primary),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "💾 {} / {}",
                                        format_bytes_to_gb(disk.used_space()),
                                        format_bytes_to_gb(disk.total_space)
                                    ))
                                    .size(11.0)
                                    .color(self.theme.text_secondary),
                                );
                            });

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:.1}%", disk.usage_percent))
                                            .size(16.0)
                                            .strong()
                                            .color(self.get_usage_color(disk.usage_percent)),
                                    );
                                },
                            );
                        });

                        ui.add_space(12.0);
                        let progress = egui::ProgressBar::new(disk.usage_percent / 100.0)
                            .desired_height(6.0)
                            .fill(self.get_usage_color(disk.usage_percent))
                            .rounding(3.0);
                        ui.add(progress);
                    });

                if i < self.disk_stats.len() - 1 {
                    ui.add_space(14.0);
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
            .inner_margin(10.0)
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
                        plot_ui.line(Line::new(points).width(2.5).color(color));
                    });
            });
    }

    fn render_card(&self, ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::none()
            .fill(self.theme.bg_card)
            .rounding(CARD_ROUNDING)
            .inner_margin(CARD_PADDING)
            .stroke(egui::Stroke::new(1.0, self.theme.border))
            .shadow(egui::epaint::Shadow {
                offset: egui::vec2(0.0, 2.0),
                blur: 12.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(40),
            })
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
