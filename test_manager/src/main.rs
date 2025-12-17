use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use sysinfo::{Disks, System};

#[cfg(all(feature = "nvidia", any(target_os = "windows", target_os = "linux")))]
use nvml_wrapper::Nvml;

const HISTORY_SIZE: usize = 120;
const REFRESH_INTERVAL_MS: u64 = 200;
const CARD_ROUNDING: f32 = 12.0;
const CARD_PADDING: f32 = 20.0;
const MAX_CONTENT_WIDTH: f32 = 1400.0;
const SIDE_PADDING: f32 = 20.0;

// ================= Theme =================

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
    fn dark() -> Self {
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

// ================= Process Tracking =================

#[derive(Clone)]
struct ProcessInfo {
    name: String,
    cpu_usage: f32,
    memory_kb: u64,
}

// ================= Disk Stats =================

#[derive(Clone)]
struct DiskInfo {
    mount_point: String,
    total_space: u64,
    available_space: u64,
    usage_percent: f32,
    file_system: String,
}

// ================= CPU Info =================

#[derive(Clone)]
struct CpuInfo {
    physical_cores: usize,
    logical_cores: usize,
    smt_enabled: bool,
    brand: String,
}

// ================= GPU Info =================

#[derive(Clone)]
struct GpuInfo {
    name: String,
    utilization: f32,
    memory_used: u64,
    memory_total: u64,
    temperature: Option<f32>,
    power_usage: Option<f32>,
}

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
        if let Some(ref nvml) = self.nvml {
            if let Ok(device_count) = nvml.device_count() {
                for i in 0..device_count {
                    if let Ok(device) = nvml.device_by_index(i) {
                        let name = device.name().unwrap_or_else(|_| format!("GPU {}", i));

                        let utilization = device
                            .utilization_rates()
                            .ok()
                            .map(|u| u.gpu as f32)
                            .unwrap_or(0.0);

                        let memory_info = device.memory_info().ok();
                        let memory_used = memory_info.as_ref().map(|m| m.used).unwrap_or(0);
                        let memory_total = memory_info.as_ref().map(|m| m.total).unwrap_or(1);

                        let temperature = device
                            .temperature(
                                nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu,
                            )
                            .ok()
                            .map(|t| t as f32);

                        let power_usage = device.power_usage().ok().map(|p| p as f32 / 1000.0); // Convert to watts

                        let gpu = GpuInfo {
                            name,
                            utilization,
                            memory_used,
                            memory_total,
                            temperature,
                            power_usage,
                        };

                        self.gpus.push(gpu);
                    }
                }
            }
        }

        // Ensure we have enough history vectors
        while self.gpu_history.len() < self.gpus.len() {
            self.gpu_history.push(VecDeque::with_capacity(HISTORY_SIZE));
        }

        // Update GPU history
        for (i, gpu) in self.gpus.iter().enumerate() {
            if let Some(history) = self.gpu_history.get_mut(i) {
                if history.len() >= HISTORY_SIZE {
                    history.pop_front();
                }
                history.push_back(gpu.utilization);
            }
        }
    }

    fn has_gpus(&self) -> bool {
        !self.gpus.is_empty()
    }
}

// ================= Layout Configuration =================

#[derive(Clone, Copy)]
enum LayoutMode {
    Desktop,
    Tablet,
    Mobile,
}

impl LayoutMode {
    fn from_width(width: f32) -> Self {
        if width >= 900.0 {
            LayoutMode::Desktop
        } else if width >= 650.0 {
            LayoutMode::Tablet
        } else {
            LayoutMode::Mobile
        }
    }

    fn stats_cards_per_row(&self) -> usize {
        match self {
            LayoutMode::Desktop => 3,
            LayoutMode::Tablet => 3,
            LayoutMode::Mobile => 1,
        }
    }

    fn show_sparklines(&self) -> bool {
        matches!(self, LayoutMode::Desktop | LayoutMode::Tablet)
    }
}

// ================= App State =================

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
    last_refresh: Instant,
    last_process_refresh: Instant,
    theme: Theme,
}

impl SystemMonitor {
    fn new() -> Self {
        let mut system = System::new_all();

        std::thread::sleep(Duration::from_millis(200));
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
            last_refresh: Instant::now(),
            last_process_refresh: Instant::now(),
            theme: Theme::dark(),
        }
    }

    fn refresh_system_stats(&mut self) {
        self.system.refresh_cpu_all();
        self.system.refresh_memory();

        // Update CPU history
        for (i, cpu) in self.system.cpus().iter().enumerate() {
            let usage = cpu.cpu_usage().clamp(0.0, 100.0);
            let history = &mut self.cpu_history[i];

            if history.len() >= HISTORY_SIZE {
                history.pop_front();
            }
            history.push_back(usage);
        }

        // Update memory history
        let mem_usage = self.memory_usage_percent();
        if self.memory_history.len() >= HISTORY_SIZE {
            self.memory_history.pop_front();
        }
        self.memory_history.push_back(mem_usage);

        // Refresh process stats less frequently (every 2 seconds)
        if self.last_process_refresh.elapsed() >= Duration::from_secs(2) {
            self.refresh_process_stats();
            self.refresh_disk_stats();
            self.gpu_monitor.refresh();
            self.last_process_refresh = Instant::now();
        }
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
            .iter()
            .map(|(_, process)| ProcessInfo {
                name: process.name().to_string_lossy().to_string(),
                cpu_usage: process.cpu_usage(),
                memory_kb: process.memory() / 1024,
            })
            .collect();

        // Sort by CPU usage
        processes.sort_by(|a, b| {
            b.cpu_usage
                .partial_cmp(&a.cpu_usage)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Keep top 5 processes
        processes.truncate(5);

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

    fn avg_cpu_usage(&self) -> f32 {
        if self.cpu_history.is_empty() {
            return 0.0;
        }
        self.cpu_history
            .iter()
            .filter_map(|h| h.back().copied())
            .sum::<f32>()
            / self.cpu_history.len() as f32
    }

    fn memory_usage_percent(&self) -> f32 {
        let total = self.system.total_memory() as f32;
        if total == 0.0 {
            return 0.0;
        }
        (self.system.used_memory() as f32 / total) * 100.0
    }

    fn memory_used_gb(&self) -> f32 {
        self.system.used_memory() as f32 / (1024.0 * 1024.0 * 1024.0)
    }

    fn memory_total_gb(&self) -> f32 {
        self.system.total_memory() as f32 / (1024.0 * 1024.0 * 1024.0)
    }

    fn format_bytes_to_gb(bytes: u64) -> String {
        let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        format!("{:.1} GB", gb)
    }
}

// ================= UI Implementation =================

impl eframe::App for SystemMonitor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Refresh system stats periodically
        if self.last_refresh.elapsed() >= Duration::from_millis(REFRESH_INTERVAL_MS) {
            self.refresh_system_stats();
            self.last_refresh = Instant::now();
        }

        // Configure visual style
        self.configure_style(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(self.theme.bg_primary))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
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
                                        self.render_cpu_section(ui, layout_mode);
                                        ui.add_space(24.0);
                                        self.render_memory_section(ui, layout_mode);
                                        ui.add_space(24.0);
                                        self.render_process_section(ui, layout_mode);
                                        ui.add_space(24.0);

                                        if self.gpu_monitor.has_gpus() {
                                            self.render_gpu_section(ui, layout_mode);
                                            ui.add_space(24.0);
                                        }

                                        self.render_disk_section(ui, layout_mode);
                                        ui.add_space(48.0);
                                    },
                                );
                            },
                        );
                    });
            });

        ctx.request_repaint_after(Duration::from_millis(REFRESH_INTERVAL_MS));
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

                        if self.cpu_info.smt_enabled {
                            ui.label(
                                egui::RichText::new("SMT: Enabled")
                                    .size(12.0)
                                    .color(self.theme.success),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new("SMT: Disabled")
                                    .size(12.0)
                                    .color(self.theme.text_secondary),
                            );
                        }
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
                + (if show_sparkline {
                    sparkline_width + 10.0
                } else {
                    0.0
                });
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
                        "{:.1} GB / {:.1} GB",
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

            let chart_height = match layout {
                LayoutMode::Desktop => 120.0,
                LayoutMode::Tablet => 100.0,
                LayoutMode::Mobile => 80.0,
            };
            self.render_chart(
                ui,
                &self.memory_history,
                "memory_chart".to_string(),
                chart_height,
            );
        });
    }

    fn render_process_section(&self, ui: &mut egui::Ui, _layout: LayoutMode) {
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
                            egui::RichText::new(format!(
                                "{:.1} MB",
                                process.memory_kb as f32 / 1024.0
                            ))
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

    fn render_gpu_section(&self, ui: &mut egui::Ui, layout: LayoutMode) {
        self.render_card(ui, |ui| {
            self.render_section_header(ui, "GPU", "Graphics processor utilization");
            ui.add_space(20.0);

            for (i, gpu) in self.gpu_monitor.gpus.iter().enumerate() {
                self.render_gpu_item(ui, i, gpu, layout);
                if i < self.gpu_monitor.gpus.len() - 1 {
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

                // VRAM usage bar
                let vram_percent = if gpu.memory_total > 0 {
                    (gpu.memory_used as f64 / gpu.memory_total as f64 * 100.0) as f32
                } else {
                    0.0
                };

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
                                "{:.1}/{:.1} GB",
                                gpu.memory_used as f64 / (1024.0 * 1024.0 * 1024.0),
                                gpu.memory_total as f64 / (1024.0 * 1024.0 * 1024.0)
                            ))
                            .size(12.0)
                            .monospace()
                            .color(self.theme.text_primary),
                        ),
                    );
                });

                ui.add_space(12.0);

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
                                        .color(if temp > 80.0 {
                                            self.theme.danger
                                        } else if temp > 70.0 {
                                            self.theme.warning
                                        } else {
                                            self.theme.text_secondary
                                        }),
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

                // GPU utilization history chart
                if let Some(history) = self.gpu_monitor.gpu_history.get(index) {
                    if !history.is_empty() {
                        ui.add_space(14.0);
                        let chart_height = match layout {
                            LayoutMode::Desktop => 90.0,
                            LayoutMode::Tablet => 80.0,
                            LayoutMode::Mobile => 70.0,
                        };
                        self.render_chart(ui, history, format!("gpu_{}", index), chart_height);
                    }
                }
            });
    }

    fn render_disk_section(&self, ui: &mut egui::Ui, _layout: LayoutMode) {
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
                    let used = disk.total_space.saturating_sub(disk.available_space);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} / {}",
                            Self::format_bytes_to_gb(used),
                            Self::format_bytes_to_gb(disk.total_space)
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

    fn get_usage_color(&self, usage: f32) -> egui::Color32 {
        if usage >= 90.0 {
            self.theme.danger
        } else if usage >= 70.0 {
            self.theme.warning
        } else {
            self.theme.accent_primary
        }
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
