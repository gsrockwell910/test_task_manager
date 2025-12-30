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
const CARD_PADDING: f32 = 20.0;
const MAX_CONTENT_WIDTH: f32 = 1400.0;
const SIDE_PADDING: f32 = 20.0;

const TOP_PROCESS_COUNT: usize = 5;
const BYTES_PER_MB: f64 = 1024.0 * 1024.0;
const BYTES_PER_GB: f64 = 1024.0 * 1024.0 * 1024.0;

const DESKTOP_BREAKPOINT: f32 = 900.0;
const TABLET_BREAKPOINT: f32 = 650.0;

// Usage thresholds
const HIGH_USAGE_THRESHOLD: f32 = 90.0;
const MEDIUM_USAGE_THRESHOLD: f32 = 70.0;
const GPU_HIGH_TEMP_THRESHOLD: f32 = 80.0;
const GPU_MEDIUM_TEMP_THRESHOLD: f32 = 70.0;

// Battery thresholds
const BATTERY_LOW_THRESHOLD: f32 = 20.0;
const BATTERY_MEDIUM_THRESHOLD: f32 = 50.0;

// ================= Theme =================

#[derive(Debug, Clone, Copy)]
struct Theme {
    bg_primary: egui::Color32,
    bg_secondary: egui::Color32,
    bg_card: egui::Color32,
    accent_primary: egui::Color32,
    accent_secondary: egui::Color32,
    text_primary: egui::Color32,
    text_secondary: egui::Color32,
    success: egui::Color32,
    warning: egui::Color32,
    danger: egui::Color32,
    border: egui::Color32,
}

impl Theme {
    const fn dark() -> Self {
        Self {
            bg_primary: egui::Color32::from_rgb(15, 17, 22),
            bg_secondary: egui::Color32::from_rgb(22, 25, 31),
            bg_card: egui::Color32::from_rgb(28, 32, 40),
            accent_primary: egui::Color32::from_rgb(88, 166, 255),
            accent_secondary: egui::Color32::from_rgb(139, 92, 246),
            text_primary: egui::Color32::from_rgb(240, 242, 245),
            text_secondary: egui::Color32::from_rgb(148, 163, 184),
            success: egui::Color32::from_rgb(34, 197, 94),
            warning: egui::Color32::from_rgb(251, 191, 36),
            danger: egui::Color32::from_rgb(239, 68, 68),
            border: egui::Color32::from_rgb(45, 50, 60),
        }
    }
}

// ================= Data Structures =================

/// Information about a running process
#[derive(Debug, Clone)]
struct ProcessInfo {
    name: String,
    cpu_usage: f32,
    memory_bytes: u64,
}

impl ProcessInfo {
    fn memory_mb(&self) -> f32 {
        self.memory_bytes as f32 / BYTES_PER_MB as f32
    }
}

/// Information about a disk/volume
#[derive(Debug, Clone)]
struct DiskInfo {
    mount_point: String,
    total_space: u64,
    available_space: u64,
    usage_percent: f32,
    file_system: String,
}

impl DiskInfo {
    fn used_space(&self) -> u64 {
        self.total_space.saturating_sub(self.available_space)
    }
}

/// CPU hardware information
#[derive(Debug, Clone)]
struct CpuInfo {
    physical_cores: usize,
    logical_cores: usize,
    smt_enabled: bool,
    brand: String,
}

/// GPU information and metrics
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
    fn memory_usage_percent(&self) -> f32 {
        if self.memory_total > 0 {
            (self.memory_used as f64 / self.memory_total as f64 * 100.0) as f32
        } else {
            0.0
        }
    }

    fn memory_used_gb(&self) -> f64 {
        self.memory_used as f64 / BYTES_PER_GB
    }

    fn memory_total_gb(&self) -> f64 {
        self.memory_total as f64 / BYTES_PER_GB
    }
}

/// Battery information
#[derive(Debug, Clone)]
struct BatteryInfo {
    percentage: f32,
    is_charging: bool,
    time_remaining: Option<Duration>,
    power_consumption: Option<f32>,
    health: Option<f32>,
}

// ================= GPU Monitor =================

/// Handles GPU monitoring with NVML support
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

    /// Refresh GPU metrics
    fn refresh(&mut self) {
        self.gpus.clear();

        #[cfg(all(feature = "nvidia", any(target_os = "windows", target_os = "linux")))]
        {
            if self.nvml.is_some() {
                self.refresh_nvidia_gpus();
            }
        }

        // Try to detect AMD/Intel GPUs
        #[cfg(target_os = "linux")]
        self.refresh_amd_gpu_linux();

        // Ensure history vectors match GPU count
        if self.gpu_history.len() != self.gpus.len() {
            self.gpu_history
                .resize(self.gpus.len(), VecDeque::with_capacity(HISTORY_SIZE));
        }

        // Update history
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

        // AMD GPUs are usually in /sys/class/drm/card*/device/
        if let Ok(entries) = fs::read_dir("/sys/class/drm") {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                // Look for card0, card1, etc (skip renderD* and card*-*)
                if !name_str.starts_with("card")
                    || name_str.contains('-')
                    || name_str.contains("renderD")
                {
                    continue;
                }

                let device_path = path.join("device");

                // Check if this is an AMD GPU
                let vendor_path = device_path.join("vendor");
                if let Ok(vendor) = fs::read_to_string(&vendor_path) {
                    let vendor = vendor.trim();
                    // 0x1002 is AMD
                    if vendor != "0x1002" {
                        continue;
                    }
                }

                // Try to read GPU name
                let name = if let Ok(product_name) =
                    fs::read_to_string(device_path.join("product_name"))
                {
                    product_name.trim().to_string()
                } else {
                    format!("AMD GPU ({})", name_str)
                };

                // Check if we already added this GPU
                if self.gpus.iter().any(|g| g.name == name) {
                    continue;
                }

                // Try to read GPU busy percentage
                let gpu_busy_path = device_path.join("gpu_busy_percent");
                let utilization = if let Ok(busy_str) = fs::read_to_string(&gpu_busy_path) {
                    busy_str.trim().parse::<f32>().unwrap_or(0.0)
                } else {
                    0.0
                };

                // Try to read VRAM usage
                let (memory_used, memory_total) = self.read_amd_vram_linux(&device_path);

                // Try to read temperature
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

                // Try to read power usage
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

                // Determine if integrated (usually has less VRAM)
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

                // Detect integrated GPU by checking:
                // 1. Name contains "integrated", "igpu", or common iGPU identifiers
                // 2. For Intel: "UHD", "Iris", "HD Graphics"
                // 3. For AMD: "Radeon Graphics" (not RX/Vega with model numbers)
                // 4. Very low VRAM (< 2GB typically indicates integrated)
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

                // Consider it integrated if memory is less than 2GB or name indicates it
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

    fn has_discrete_gpu(&self) -> bool {
        self.gpus.iter().any(|gpu| !gpu.is_integrated)
    }

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

                    // Update history
                    if self.charge_history.len() >= HISTORY_SIZE {
                        self.charge_history.pop_front();
                    }
                    self.charge_history.push_back(percentage);
                }
            }
        }
    }

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

    fn stats_cards_per_row(self) -> usize {
        match self {
            Self::Desktop | Self::Tablet => 3,
            Self::Mobile => 1,
        }
    }

    fn show_sparklines(self) -> bool {
        matches!(self, Self::Desktop | Self::Tablet)
    }

    fn chart_height(self) -> f32 {
        match self {
            Self::Desktop => 120.0,
            Self::Tablet => 100.0,
            Self::Mobile => 80.0,
        }
    }

    fn gpu_chart_height(self) -> f32 {
        match self {
            Self::Desktop => 90.0,
            Self::Tablet => 80.0,
            Self::Mobile => 70.0,
        }
    }
}

// ================= System Monitor =================

/// Main application state
struct SystemMonitor {
    // System monitoring
    system: System,
    disks: Disks,

    // Metrics history
    cpu_history: Vec<VecDeque<f32>>,
    memory_history: VecDeque<f32>,

    // Cached data
    top_processes: Vec<ProcessInfo>,
    total_processes: usize,
    disk_stats: Vec<DiskInfo>,
    cpu_info: CpuInfo,

    // GPU monitoring
    gpu_monitor: GpuMonitor,

    // Battery monitoring
    battery_monitor: BatteryMonitor,

    // Timing
    last_fast_refresh: Instant,
    last_slow_refresh: Instant,

    // UI
    theme: Theme,
}

impl SystemMonitor {
    fn new() -> Self {
        let mut system = System::new_all();

        // Initial CPU sampling for accurate readings
        std::thread::sleep(INITIAL_CPU_SAMPLE_DELAY);
        system.refresh_cpu_all();
        system.refresh_memory();

        let cpu_count = system.cpus().len();
        let physical_cores = system.physical_core_count().unwrap_or(cpu_count);
        let smt_enabled = cpu_count > physical_cores;

        let cpu_brand = system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().to_string())
            .unwrap_or_else(|| "Unknown CPU".to_string());

        let cpu_info = CpuInfo {
            physical_cores,
            logical_cores: cpu_count,
            smt_enabled,
            brand: cpu_brand,
        };

        let disks = Disks::new_with_refreshed_list();

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
            theme: Theme::dark(),
        }
    }

    /// Refresh fast-changing metrics (CPU, memory)
    fn refresh_fast_metrics(&mut self) {
        self.system.refresh_cpu_all();
        self.system.refresh_memory();

        // Update CPU history
        for (i, cpu) in self.system.cpus().iter().enumerate() {
            let usage = cpu.cpu_usage().clamp(0.0, 100.0);

            if let Some(history) = self.cpu_history.get_mut(i) {
                if history.len() >= HISTORY_SIZE {
                    history.pop_front();
                }
                history.push_back(usage);
            }
        }

        // Update memory history
        let mem_usage = self.memory_usage_percent();
        if self.memory_history.len() >= HISTORY_SIZE {
            self.memory_history.pop_front();
        }
        self.memory_history.push_back(mem_usage);
    }

    /// Refresh slow-changing metrics (processes, disks, GPU, battery)
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

        // Pre-allocate with capacity to avoid reallocations
        let mut processes = Vec::with_capacity(TOP_PROCESS_COUNT * 2);

        processes.extend(self.system.processes().values().map(|process| ProcessInfo {
            name: process.name().to_string_lossy().into_owned(),
            cpu_usage: process.cpu_usage(),
            memory_bytes: process.memory(),
        }));

        // Use unstable sort for better performance (order of equal elements doesn't matter)
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
                    file_system: disk.file_system().to_string_lossy().to_string(),
                }
            })
            .collect();
    }

    // Metric accessors
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
            self.theme.accent_primary
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
    format!("{:.2} GB", bytes as f64 / BYTES_PER_GB)
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

// ================= UI Implementation =================

impl eframe::App for SystemMonitor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Fast refresh (CPU, memory)
        if self.last_fast_refresh.elapsed() >= FAST_REFRESH_INTERVAL {
            self.refresh_fast_metrics();
            self.last_fast_refresh = Instant::now();
        }

        // Slow refresh (processes, disks, GPU, battery)
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

        ctx.request_repaint_after(FAST_REFRESH_INTERVAL);
    }
}

impl SystemMonitor {
    fn configure_style(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.visuals.widgets.noninteractive.bg_fill = self.theme.bg_card;
        style.visuals.widgets.inactive.bg_fill = self.theme.bg_card;
        style.visuals.extreme_bg_color = self.theme.bg_secondary;
        style.spacing.scroll = egui::style::ScrollStyle {
            bar_width: 6.0,
            ..Default::default()
        };
        ctx.set_style(style);
    }

    fn render_content(&mut self, ui: &mut egui::Ui) {
        let available_width = ui.available_width();
        let content_width = (available_width - (SIDE_PADDING * 2.0))
            .min(MAX_CONTENT_WIDTH)
            .max(300.0);
        let layout_mode = LayoutMode::from_width(content_width);

        ui.allocate_ui_with_layout(
            egui::vec2(available_width, ui.available_height()),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.add_space(24.0);

                ui.allocate_ui_with_layout(
                    egui::vec2(content_width, ui.available_height()),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                        ui.set_max_width(content_width);

                        self.render_header(ui);
                        ui.add_space(32.0);

                        self.render_stats_overview(ui, layout_mode);
                        ui.add_space(24.0);

                        // Battery section (if available)
                        if self.battery_monitor.has_battery() {
                            self.render_battery_section(ui, layout_mode);
                            ui.add_space(24.0);
                        }

                        self.render_cpu_section(ui, layout_mode);
                        ui.add_space(24.0);

                        self.render_memory_section(ui, layout_mode);
                        ui.add_space(24.0);

                        self.render_process_section(ui);
                        ui.add_space(24.0);

                        // Integrated GPU section
                        if self.gpu_monitor.has_integrated_gpu() {
                            self.render_igpu_section(ui, layout_mode);
                            ui.add_space(24.0);
                        }

                        // Discrete GPU section
                        if self.gpu_monitor.has_discrete_gpu() {
                            self.render_dgpu_section(ui, layout_mode);
                            ui.add_space(24.0);
                        }

                        self.render_disk_section(ui);
                        ui.add_space(48.0);
                    },
                );
            },
        );
    }

    fn render_header(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("System Monitor")
                    .size(36.0)
                    .strong()
                    .color(self.theme.text_primary),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Real-time performance metrics")
                    .size(15.0)
                    .color(self.theme.text_secondary),
            );
        });
    }

    fn render_stats_overview(&self, ui: &mut egui::Ui, layout: LayoutMode) {
        let cards_per_row = layout.stats_cards_per_row();
        let process_count = self.total_processes;

        if cards_per_row == 1 {
            let width = ui.available_width();
            self.render_stat_card(
                ui,
                width,
                "CPU",
                format!("{:.1}%", self.avg_cpu_usage()),
                self.theme.accent_primary,
            );
            ui.add_space(12.0);

            self.render_stat_card(
                ui,
                width,
                "Memory",
                format!("{:.1}%", self.memory_usage_percent()),
                self.theme.accent_secondary,
            );
            ui.add_space(12.0);

            self.render_stat_card(
                ui,
                width,
                "Processes",
                process_count.to_string(),
                self.theme.success,
            );
        } else {
            let available = ui.available_width();
            let spacing = 12.0;
            let total_spacing = spacing * (cards_per_row - 1) as f32;
            let card_width = (available - total_spacing) / cards_per_row as f32;

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = spacing;

                self.render_stat_card(
                    ui,
                    card_width,
                    "CPU",
                    format!("{:.1}%", self.avg_cpu_usage()),
                    self.theme.accent_primary,
                );

                self.render_stat_card(
                    ui,
                    card_width,
                    "Memory",
                    format!("{:.1}%", self.memory_usage_percent()),
                    self.theme.accent_secondary,
                );

                self.render_stat_card(
                    ui,
                    card_width,
                    "Processes",
                    process_count.to_string(),
                    self.theme.success,
                );
            });
        }
    }

    fn render_stat_card(
        &self,
        ui: &mut egui::Ui,
        width: f32,
        label: &str,
        value: String,
        color: egui::Color32,
    ) {
        egui::Frame::none()
            .fill(self.theme.bg_card)
            .rounding(CARD_ROUNDING)
            .inner_margin(CARD_PADDING)
            .stroke(egui::Stroke::new(1.0, self.theme.border))
            .show(ui, |ui| {
                ui.set_width(width - (CARD_PADDING * 2.0) - 2.0);
                ui.set_min_height(85.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(label)
                            .size(13.0)
                            .color(self.theme.text_secondary),
                    );
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(value).size(30.0).strong().color(color));
                });
            });
    }

    fn render_battery_section(&self, ui: &mut egui::Ui, layout: LayoutMode) {
        if let Some(battery) = &self.battery_monitor.battery_info {
            self.render_card(ui, |ui| {
                self.render_section_header(ui, "Battery", "Power status and charge level");
                ui.add_space(20.0);

                // Main battery display container
                egui::Frame::none()
                    .fill(self.theme.bg_secondary)
                    .rounding(10.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Left side: Battery icon and percentage
                            ui.vertical(|ui| {
                                ui.add_space(4.0);

                                let battery_icon = if battery.is_charging {
                                    "⚡"
                                } else if battery.percentage > 80.0 {
                                    "🔋"
                                } else if battery.percentage > 50.0 {
                                    "🔋"
                                } else if battery.percentage > 20.0 {
                                    "🪫"
                                } else {
                                    "🪫"
                                };

                                ui.label(egui::RichText::new(battery_icon).size(48.0));
                            });

                            ui.add_space(16.0);

                            // Right side: Details
                            ui.vertical(|ui| {
                                let battery_color = self.get_battery_color(battery.percentage);

                                // Percentage
                                ui.label(
                                    egui::RichText::new(format!("{:.0}%", battery.percentage))
                                        .size(32.0)
                                        .strong()
                                        .color(battery_color),
                                );

                                ui.add_space(4.0);

                                // Status
                                let (status_text, status_color) = if battery.is_charging {
                                    ("Charging", self.theme.success)
                                } else {
                                    ("On Battery", self.theme.text_secondary)
                                };

                                ui.label(
                                    egui::RichText::new(status_text)
                                        .size(14.0)
                                        .color(status_color),
                                );

                                // Time remaining
                                if let Some(time) = battery.time_remaining {
                                    ui.add_space(2.0);
                                    let prefix = if battery.is_charging {
                                        "Full in"
                                    } else {
                                        "Remaining"
                                    };
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{}: {}",
                                            prefix,
                                            format_duration(time)
                                        ))
                                        .size(13.0)
                                        .color(self.theme.text_secondary),
                                    );
                                }
                            });

                            // Spacer
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                ui.add_space(0.0);
                            });
                        });
                    });

                ui.add_space(16.0);

                // Battery level progress bar
                let battery_color = self.get_battery_color(battery.percentage);

                ui.add(
                    egui::ProgressBar::new(battery.percentage / 100.0)
                        .desired_width(ui.available_width())
                        .desired_height(12.0)
                        .fill(battery_color)
                        .rounding(6.0),
                );

                ui.add_space(16.0);

                // Additional metrics grid
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);

                    // Power consumption
                    if let Some(power) = battery.power_consumption {
                        self.render_battery_metric(
                            ui,
                            "Power Draw",
                            &format!("{:.1} W", power),
                            "⚡",
                        );
                    }

                    // Battery health
                    if let Some(health) = battery.health {
                        let health_icon = if health > 90.0 {
                            "💚"
                        } else if health > 70.0 {
                            "💛"
                        } else {
                            "❤️"
                        };

                        self.render_battery_metric(
                            ui,
                            "Health",
                            &format!("{:.0}%", health),
                            health_icon,
                        );
                    }

                    // Charging cycles (placeholder - could be added if available)
                    if battery.is_charging {
                        self.render_battery_metric(ui, "Status", "AC Power", "🔌");
                    }
                });

                // Battery charge history chart
                if !self.battery_monitor.charge_history.is_empty()
                    && self.battery_monitor.charge_history.len() > 1
                {
                    ui.add_space(20.0);

                    ui.label(
                        egui::RichText::new("Charge History")
                            .size(13.0)
                            .color(self.theme.text_secondary),
                    );

                    ui.add_space(8.0);

                    let chart_height = layout.chart_height();
                    self.render_chart(
                        ui,
                        &self.battery_monitor.charge_history,
                        "battery_chart".to_string(),
                        chart_height,
                    );
                }
            });
        }
    }

    fn render_battery_metric(&self, ui: &mut egui::Ui, label: &str, value: &str, icon: &str) {
        egui::Frame::none()
            .fill(self.theme.bg_card)
            .rounding(8.0)
            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon).size(16.0));
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(
                            egui::RichText::new(label)
                                .size(11.0)
                                .color(self.theme.text_secondary),
                        );
                        ui.label(
                            egui::RichText::new(value)
                                .size(14.0)
                                .strong()
                                .color(self.theme.text_primary),
                        );
                    });
                });
            });
    }

    fn render_cpu_section(&self, ui: &mut egui::Ui, layout: LayoutMode) {
        self.render_card(ui, |ui| {
            self.render_section_header(ui, "CPU", "Processor utilization");
            ui.add_space(12.0);

            // CPU Info
            egui::Frame::none()
                .fill(self.theme.bg_secondary)
                .rounding(8.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(&self.cpu_info.brand)
                            .size(13.0)
                            .color(self.theme.text_secondary),
                    );
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;

                        ui.label(
                            egui::RichText::new(format!(
                                "Physical: {} cores",
                                self.cpu_info.physical_cores
                            ))
                            .size(12.0)
                            .color(self.theme.text_secondary),
                        );

                        ui.label(
                            egui::RichText::new(format!(
                                "Logical: {} CPUs",
                                self.cpu_info.logical_cores
                            ))
                            .size(12.0)
                            .color(self.theme.text_secondary),
                        );

                        let smt_text = if self.cpu_info.smt_enabled {
                            "SMT: Enabled"
                        } else {
                            "SMT: Disabled"
                        };

                        let smt_color = if self.cpu_info.smt_enabled {
                            self.theme.success
                        } else {
                            self.theme.text_secondary
                        };

                        ui.label(egui::RichText::new(smt_text).size(12.0).color(smt_color));
                    });
                });

            ui.add_space(20.0);

            for (i, history) in self.cpu_history.iter().enumerate() {
                let current = history.back().copied().unwrap_or(0.0);
                self.render_cpu_core(ui, i, current, history, layout);
                ui.add_space(10.0);
            }
        });
    }

    fn render_cpu_core(
        &self,
        ui: &mut egui::Ui,
        core_id: usize,
        usage: f32,
        history: &VecDeque<f32>,
        layout: LayoutMode,
    ) {
        let show_sparkline = layout.show_sparklines();
        let available = ui.available_width();

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;

            ui.add_sized(
                [70.0, 24.0],
                egui::Label::new(
                    egui::RichText::new(format!("Core {}", core_id))
                        .size(13.0)
                        .color(self.theme.text_secondary),
                ),
            );

            let sparkline_width = if show_sparkline {
                (available * 0.25).max(100.0).min(200.0)
            } else {
                0.0
            };

            let used_width = 70.0
                + 55.0
                + if show_sparkline {
                    sparkline_width + 10.0
                } else {
                    0.0
                };

            let spacing_count = if show_sparkline { 3.0 } else { 2.0 };
            let progress_width = (available - used_width - (10.0 * spacing_count)).max(100.0);

            ui.add_sized(
                [progress_width, 24.0],
                egui::ProgressBar::new(usage / 100.0)
                    .fill(self.get_usage_color(usage))
                    .rounding(6.0),
            );

            ui.add_sized(
                [55.0, 24.0],
                egui::Label::new(
                    egui::RichText::new(format!("{:.1}%", usage))
                        .size(13.0)
                        .monospace()
                        .color(self.theme.text_primary),
                ),
            );

            if show_sparkline {
                self.render_sparkline(
                    ui,
                    history,
                    format!("cpu_{}", core_id),
                    self.theme.accent_primary,
                    sparkline_width,
                );
            }
        });
    }

    fn render_memory_section(&self, ui: &mut egui::Ui, layout: LayoutMode) {
        self.render_card(ui, |ui| {
            self.render_section_header(ui, "Memory", "RAM utilization");
            ui.add_space(20.0);

            let usage_pct = self.memory_usage_percent();

            ui.add(
                egui::ProgressBar::new(usage_pct / 100.0)
                    .desired_width(ui.available_width())
                    .desired_height(32.0)
                    .fill(self.get_usage_color(usage_pct))
                    .rounding(8.0),
            );

            ui.add_space(12.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{:.2} GB / {:.2} GB",
                        self.memory_used_gb(),
                        self.memory_total_gb()
                    ))
                    .size(14.0)
                    .color(self.theme.text_secondary),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{:.1}%", usage_pct))
                            .size(15.0)
                            .strong()
                            .color(self.theme.text_primary),
                    );
                });
            });

            ui.add_space(16.0);

            let chart_height = layout.chart_height();
            self.render_chart(
                ui,
                &self.memory_history,
                "memory_chart".to_string(),
                chart_height,
            );
        });
    }

    fn render_process_section(&self, ui: &mut egui::Ui) {
        self.render_card(ui, |ui| {
            self.render_section_header(ui, "Processes", "Active system processes");
            ui.add_space(20.0);

            ui.label(
                egui::RichText::new(format!("Total: {} processes", self.total_processes))
                    .size(13.0)
                    .color(self.theme.text_secondary),
            );

            ui.add_space(16.0);

            ui.label(
                egui::RichText::new("Top CPU Consumers")
                    .size(14.0)
                    .strong()
                    .color(self.theme.text_primary),
            );
            ui.add_space(10.0);

            if self.top_processes.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("No process data available")
                            .size(13.0)
                            .color(self.theme.text_secondary),
                    );
                    ui.add_space(20.0);
                });
            } else {
                for process in &self.top_processes {
                    self.render_process_item(ui, process);
                    ui.add_space(8.0);
                }
            }
        });
    }

    fn render_process_item(&self, ui: &mut egui::Ui, process: &ProcessInfo) {
        egui::Frame::none()
            .fill(self.theme.bg_secondary)
            .rounding(6.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let max_name_width = ui.available_width() * 0.6;
                    ui.allocate_ui_with_layout(
                        egui::vec2(max_name_width, 20.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.label(
                                egui::RichText::new(&process.name)
                                    .size(13.0)
                                    .color(self.theme.text_primary),
                            );
                        },
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{:.1} MB", process.memory_mb()))
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );

                        ui.add_space(12.0);

                        ui.label(
                            egui::RichText::new(format!("{:.1}%", process.cpu_usage))
                                .size(12.0)
                                .strong()
                                .color(self.theme.accent_primary),
                        );
                    });
                });
            });
    }

    fn render_igpu_section(&self, ui: &mut egui::Ui, layout: LayoutMode) {
        self.render_card(ui, |ui| {
            // Header with icon
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🎮").size(24.0));
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        egui::RichText::new("Integrated GPU")
                            .size(20.0)
                            .strong()
                            .color(self.theme.text_primary),
                    );
                    ui.label(
                        egui::RichText::new("Built-in graphics processor")
                            .size(13.0)
                            .color(self.theme.text_secondary),
                    );
                });
            });

            ui.add_space(20.0);

            let igpus = self.gpu_monitor.get_integrated_gpus();

            if igpus.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(egui::RichText::new("🔍").size(32.0));
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("No integrated GPU detected")
                            .size(13.0)
                            .color(self.theme.text_secondary),
                    );
                    ui.add_space(20.0);
                });
                return;
            }

            for (i, gpu) in igpus.iter().enumerate() {
                let gpu_index = self
                    .gpu_monitor
                    .gpus
                    .iter()
                    .position(|g| g.name == gpu.name && g.is_integrated)
                    .unwrap_or(i);

                self.render_igpu_item(ui, gpu_index, gpu, layout);
                if i < igpus.len() - 1 {
                    ui.add_space(16.0);
                }
            }
        });
    }

    fn render_igpu_item(&self, ui: &mut egui::Ui, index: usize, gpu: &GpuInfo, layout: LayoutMode) {
        egui::Frame::none()
            .fill(self.theme.bg_secondary)
            .rounding(10.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                // GPU name and model
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("💎").size(20.0));
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(&gpu.name)
                            .size(16.0)
                            .strong()
                            .color(self.theme.text_primary),
                    );
                });

                ui.add_space(16.0);

                // Check if we have utilization data
                let has_utilization = gpu.utilization > 0.0 || gpu.memory_total > 0;

                if has_utilization {
                    // Main metrics grid
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);

                        // GPU Utilization metric card
                        self.render_igpu_metric_card(
                            ui,
                            "GPU Usage",
                            &format!("{:.0}%", gpu.utilization),
                            "📊",
                            self.get_usage_color(gpu.utilization),
                            Some(gpu.utilization),
                        );

                        // VRAM metric card (if available)
                        if gpu.memory_total > 0 {
                            let vram_percent = gpu.memory_usage_percent();
                            self.render_igpu_metric_card(
                                ui,
                                "VRAM",
                                &format!(
                                    "{:.2}/{:.2} GB",
                                    gpu.memory_used_gb(),
                                    gpu.memory_total_gb()
                                ),
                                "💾",
                                self.get_usage_color(vram_percent),
                                Some(vram_percent),
                            );
                        }

                        // Temperature metric card
                        if let Some(temp) = gpu.temperature {
                            self.render_igpu_metric_card(
                                ui,
                                "Temperature",
                                &format!("{:.0}°C", temp),
                                "🌡️",
                                self.get_temp_color(temp),
                                None,
                            );
                        }

                        // Power usage metric card
                        if let Some(power) = gpu.power_usage {
                            self.render_igpu_metric_card(
                                ui,
                                "Power Draw",
                                &format!("{:.1}W", power),
                                "⚡",
                                self.theme.text_secondary,
                                None,
                            );
                        }
                    });

                    // GPU utilization history chart
                    if let Some(history) = self.gpu_monitor.gpu_history.get(index) {
                        if !history.is_empty() && history.len() > 1 {
                            ui.add_space(20.0);

                            ui.label(
                                egui::RichText::new("Utilization History")
                                    .size(13.0)
                                    .color(self.theme.text_secondary),
                            );

                            ui.add_space(8.0);

                            let chart_height = layout.gpu_chart_height();
                            self.render_chart(ui, history, format!("igpu_{}", index), chart_height);
                        }
                    }
                } else {
                    // No utilization data available
                    egui::Frame::none()
                        .fill(self.theme.bg_card)
                        .rounding(8.0)
                        .inner_margin(16.0)
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new("⚠️").size(24.0));
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("Limited monitoring available")
                                        .size(13.0)
                                        .color(self.theme.warning),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(
                                        "GPU detected but utilization data unavailable",
                                    )
                                    .size(11.0)
                                    .color(self.theme.text_secondary),
                                );
                            });
                        });

                    // Show temperature if available
                    if let Some(temp) = gpu.temperature {
                        ui.add_space(12.0);
                        ui.horizontal_wrapped(|ui| {
                            self.render_igpu_metric_card(
                                ui,
                                "Temperature",
                                &format!("{:.0}°C", temp),
                                "🌡️",
                                self.get_temp_color(temp),
                                None,
                            );
                        });
                    }
                }
            });
    }

    fn render_igpu_metric_card(
        &self,
        ui: &mut egui::Ui,
        label: &str,
        value: &str,
        icon: &str,
        value_color: egui::Color32,
        progress_value: Option<f32>,
    ) {
        let width = (ui.available_width() - 24.0) / 2.0;
        let width = width.max(140.0).min(220.0);

        egui::Frame::none()
            .fill(self.theme.bg_card)
            .rounding(8.0)
            .inner_margin(egui::Margin::symmetric(12.0, 12.0))
            .show(ui, |ui| {
                ui.set_width(width - 24.0);
                ui.set_min_height(70.0);

                ui.vertical(|ui| {
                    // Icon and label
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(icon).size(16.0));
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(label)
                                .size(11.0)
                                .color(self.theme.text_secondary),
                        );
                    });

                    ui.add_space(8.0);

                    // Value
                    ui.label(
                        egui::RichText::new(value)
                            .size(18.0)
                            .strong()
                            .color(value_color),
                    );

                    // Progress bar if provided
                    if let Some(progress) = progress_value {
                        ui.add_space(8.0);
                        ui.add(
                            egui::ProgressBar::new(progress / 100.0)
                                .desired_width(ui.available_width())
                                .desired_height(6.0)
                                .fill(value_color)
                                .rounding(3.0),
                        );
                    }
                });
            });
    }

    fn render_dgpu_section(&self, ui: &mut egui::Ui, layout: LayoutMode) {
        self.render_card(ui, |ui| {
            self.render_section_header(ui, "Discrete GPU", "Graphics processor utilization");
            ui.add_space(20.0);

            let dgpus = self.gpu_monitor.get_discrete_gpus();
            for (i, gpu) in dgpus.iter().enumerate() {
                let gpu_index = self
                    .gpu_monitor
                    .gpus
                    .iter()
                    .position(|g| g.name == gpu.name && !g.is_integrated)
                    .unwrap_or(i);

                self.render_gpu_item(ui, gpu_index, gpu, layout);
                if i < dgpus.len() - 1 {
                    ui.add_space(16.0);
                }
            }
        });
    }

    fn render_gpu_item(&self, ui: &mut egui::Ui, index: usize, gpu: &GpuInfo, layout: LayoutMode) {
        egui::Frame::none()
            .fill(self.theme.bg_secondary)
            .rounding(8.0)
            .inner_margin(14.0)
            .show(ui, |ui| {
                // GPU name header
                ui.label(
                    egui::RichText::new(&gpu.name)
                        .size(15.0)
                        .strong()
                        .color(self.theme.text_primary),
                );

                ui.add_space(12.0);

                // Check if we have utilization data
                let has_utilization = gpu.utilization > 0.0 || gpu.memory_total > 0;

                if has_utilization {
                    // GPU utilization bar
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("GPU:")
                                .size(13.0)
                                .color(self.theme.text_secondary),
                        );

                        let available = ui.available_width() - 60.0;
                        ui.add_sized(
                            [available, 24.0],
                            egui::ProgressBar::new(gpu.utilization / 100.0)
                                .fill(self.get_usage_color(gpu.utilization))
                                .rounding(6.0),
                        );

                        ui.add_sized(
                            [50.0, 24.0],
                            egui::Label::new(
                                egui::RichText::new(format!("{:.0}%", gpu.utilization))
                                    .size(13.0)
                                    .monospace()
                                    .color(self.theme.text_primary),
                            ),
                        );
                    });

                    ui.add_space(10.0);

                    // VRAM usage bar (only if we have memory info)
                    if gpu.memory_total > 0 {
                        let vram_percent = gpu.memory_usage_percent();

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("VRAM:")
                                    .size(13.0)
                                    .color(self.theme.text_secondary),
                            );

                            let available = ui.available_width() - 110.0;
                            ui.add_sized(
                                [available, 24.0],
                                egui::ProgressBar::new(vram_percent / 100.0)
                                    .fill(self.get_usage_color(vram_percent))
                                    .rounding(6.0),
                            );

                            ui.add_sized(
                                [105.0, 24.0],
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "{:.2}/{:.2} GB",
                                        gpu.memory_used_gb(),
                                        gpu.memory_total_gb()
                                    ))
                                    .size(12.0)
                                    .monospace()
                                    .color(self.theme.text_primary),
                                ),
                            );
                        });

                        ui.add_space(12.0);
                    }
                } else {
                    // No utilization data available
                    egui::Frame::none()
                        .fill(self.theme.bg_card)
                        .rounding(6.0)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("⚠ GPU utilization data not available")
                                    .size(12.0)
                                    .color(self.theme.warning),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("Temperature monitoring only")
                                    .size(11.0)
                                    .color(self.theme.text_secondary),
                            );
                        });

                    ui.add_space(12.0);
                }

                // Additional stats
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;

                    if let Some(temp) = gpu.temperature {
                        egui::Frame::none()
                            .fill(self.theme.bg_card)
                            .rounding(6.0)
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format!("Temp: {:.0}°C", temp))
                                        .size(12.0)
                                        .color(self.get_temp_color(temp)),
                                );
                            });
                    }

                    if let Some(power) = gpu.power_usage {
                        egui::Frame::none()
                            .fill(self.theme.bg_card)
                            .rounding(6.0)
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format!("Power: {:.0}W", power))
                                        .size(12.0)
                                        .color(self.theme.text_secondary),
                                );
                            });
                    }
                });

                // GPU utilization history chart (only if we have utilization data)
                if has_utilization {
                    if let Some(history) = self.gpu_monitor.gpu_history.get(index) {
                        if !history.is_empty() {
                            ui.add_space(14.0);
                            let chart_height = layout.gpu_chart_height();
                            self.render_chart(ui, history, format!("gpu_{}", index), chart_height);
                        }
                    }
                }
            });
    }

    fn render_disk_section(&self, ui: &mut egui::Ui) {
        self.render_card(ui, |ui| {
            self.render_section_header(ui, "Storage", "Disk usage");
            ui.add_space(20.0);

            if self.disk_stats.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("No disks detected")
                            .size(14.0)
                            .color(self.theme.text_secondary),
                    );
                    ui.add_space(20.0);
                });
                return;
            }

            for disk in &self.disk_stats {
                self.render_disk_item(ui, disk);
                ui.add_space(16.0);
            }
        });
    }

    fn render_disk_item(&self, ui: &mut egui::Ui, disk: &DiskInfo) {
        egui::Frame::none()
            .fill(self.theme.bg_secondary)
            .rounding(8.0)
            .inner_margin(14.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&disk.mount_point)
                            .size(14.0)
                            .strong()
                            .color(self.theme.text_primary),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(&disk.file_system)
                                .size(11.0)
                                .color(self.theme.text_secondary),
                        );
                    });
                });

                ui.add_space(10.0);

                ui.add(
                    egui::ProgressBar::new(disk.usage_percent / 100.0)
                        .desired_width(ui.available_width())
                        .desired_height(24.0)
                        .fill(self.get_usage_color(disk.usage_percent))
                        .rounding(6.0),
                );

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} / {}",
                            format_bytes_to_gb(disk.used_space()),
                            format_bytes_to_gb(disk.total_space)
                        ))
                        .size(12.0)
                        .color(self.theme.text_secondary),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{:.1}% used", disk.usage_percent))
                                .size(12.0)
                                .color(self.theme.text_primary),
                        );
                    });
                });
            });
    }

    fn render_sparkline(
        &self,
        ui: &mut egui::Ui,
        history: &VecDeque<f32>,
        id: String,
        color: egui::Color32,
        width: f32,
    ) {
        let points: PlotPoints = history
            .iter()
            .enumerate()
            .map(|(x, y)| [x as f64, *y as f64])
            .collect();

        Plot::new(id)
            .height(24.0)
            .width(width)
            .allow_scroll(false)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_boxed_zoom(false)
            .show_axes([false, false])
            .show_background(false)
            .show(ui, |plot_ui| {
                plot_ui.line(Line::new(points).width(1.5).color(color));
            });
    }

    fn render_chart(&self, ui: &mut egui::Ui, history: &VecDeque<f32>, id: String, height: f32) {
        let points: PlotPoints = history
            .iter()
            .enumerate()
            .map(|(x, y)| [x as f64, *y as f64])
            .collect();

        Plot::new(id)
            .height(height)
            .width(ui.available_width())
            .allow_scroll(false)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_boxed_zoom(false)
            .show_axes([false, true])
            .show_grid([false, true])
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new(points)
                        .width(2.5)
                        .color(self.theme.accent_primary),
                );
            });
    }

    fn render_card(&self, ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::none()
            .fill(self.theme.bg_card)
            .rounding(CARD_ROUNDING)
            .inner_margin(CARD_PADDING)
            .stroke(egui::Stroke::new(1.0, self.theme.border))
            .show(ui, add_contents);
    }

    fn render_section_header(&self, ui: &mut egui::Ui, title: &str, subtitle: &str) {
        ui.label(
            egui::RichText::new(title)
                .size(20.0)
                .strong()
                .color(self.theme.text_primary),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(subtitle)
                .size(13.0)
                .color(self.theme.text_secondary),
        );
    }
}

// ================= Main Entry Point =================

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 800.0])
            .with_min_inner_size([500.0, 600.0])
            .with_title("System Monitor Pro"),
        ..Default::default()
    };

    eframe::run_native(
        "System Monitor Pro",
        options,
        Box::new(|_cc| Ok(Box::new(SystemMonitor::new()))),
    )
}
