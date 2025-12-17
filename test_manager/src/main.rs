use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use rand::{Rng, SeedableRng, rngs::StdRng};
use sysinfo::System;
use tokio::time::sleep;

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

// ================= Metrics =================

#[derive(Clone, Default)]
struct Metrics {
    inner: Arc<RwLock<HashMap<String, JobStats>>>,
}

#[derive(Clone, Default)]
struct JobStats {
    total_runs: u64,
    successful_runs: u64,
    total_duration: Duration,
    latency_history: VecDeque<f32>,
}

impl JobStats {
    fn avg_latency_ms(&self) -> f32 {
        if self.total_runs == 0 {
            return 0.0;
        }
        self.total_duration.as_millis() as f32 / self.total_runs as f32
    }

    fn success_rate(&self) -> f32 {
        if self.total_runs == 0 {
            return 0.0;
        }
        (self.successful_runs as f32 * 100.0) / self.total_runs as f32
    }

    fn current_latency_ms(&self) -> f32 {
        self.latency_history.back().copied().unwrap_or(0.0)
    }

    fn min_latency_ms(&self) -> f32 {
        self.latency_history
            .iter()
            .copied()
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0)
    }

    fn max_latency_ms(&self) -> f32 {
        self.latency_history
            .iter()
            .copied()
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0)
    }
}

impl Metrics {
    fn record(&self, job_name: &str, duration: Duration, success: bool) {
        let mut metrics = self.inner.write().unwrap();
        let stats = metrics.entry(job_name.to_string()).or_default();

        stats.total_runs += 1;
        if success {
            stats.successful_runs += 1;
        }
        stats.total_duration += duration;

        if stats.latency_history.len() >= HISTORY_SIZE {
            stats.latency_history.pop_front();
        }
        stats.latency_history.push_back(duration.as_millis() as f32);
    }

    fn get_snapshot(&self) -> HashMap<String, JobStats> {
        self.inner.read().unwrap().clone()
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
    metrics: Metrics,
    system: System,
    cpu_history: Vec<VecDeque<f32>>,
    memory_history: VecDeque<f32>,
    last_refresh: Instant,
    theme: Theme,
}

impl SystemMonitor {
    fn new(metrics: Metrics) -> Self {
        let mut system = System::new_all();
        system.refresh_cpu();
        let cpu_count = system.cpus().len();

        Self {
            metrics,
            system,
            cpu_history: vec![VecDeque::with_capacity(HISTORY_SIZE); cpu_count],
            memory_history: VecDeque::with_capacity(HISTORY_SIZE),
            last_refresh: Instant::now(),
            theme: Theme::dark(),
        }
    }

    fn refresh_system_stats(&mut self) {
        self.system.refresh_cpu();
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
        self.system.used_memory() as f32 / 1_048_576.0
    }

    fn memory_total_gb(&self) -> f32 {
        self.system.total_memory() as f32 / 1_048_576.0
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
                        // Calculate responsive layout with proper margins
                        let available_width = ui.available_width();
                        let content_width = (available_width - (SIDE_PADDING * 2.0))
                            .min(MAX_CONTENT_WIDTH)
                            .max(300.0);
                        let layout_mode = LayoutMode::from_width(content_width);

                        // Center and constrain content
                        ui.allocate_ui_with_layout(
                            egui::vec2(available_width, ui.available_height()),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add_space(24.0);

                                // Main content container with fixed width
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
                                        self.render_jobs_section(ui, layout_mode);
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

        // Smooth scrolling
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
        let job_count = self.metrics.get_snapshot().len();

        if cards_per_row == 1 {
            // Mobile: Stack vertically
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
                "Active Jobs",
                job_count.to_string(),
                self.theme.success,
            );
        } else {
            // Desktop/Tablet: Horizontal layout
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
                    "Active Jobs",
                    job_count.to_string(),
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
            self.render_section_header(ui, "CPU Cores", "Per-core utilization");
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

            // Core label - fixed width
            ui.add_sized(
                [70.0, 24.0],
                egui::Label::new(
                    egui::RichText::new(format!("Core {}", core_id))
                        .size(13.0)
                        .color(self.theme.text_secondary),
                ),
            );

            // Calculate remaining space for progress bar
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

            // Progress bar
            ui.add_sized(
                [progress_width, 24.0],
                egui::ProgressBar::new(usage / 100.0)
                    .fill(self.get_usage_color(usage))
                    .rounding(6.0),
            );

            // Percentage - fixed width
            ui.add_sized(
                [55.0, 24.0],
                egui::Label::new(
                    egui::RichText::new(format!("{:.1}%", usage))
                        .size(13.0)
                        .monospace()
                        .color(self.theme.text_primary),
                ),
            );

            // Sparkline (only on larger screens)
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

            // Memory bar
            ui.add(
                egui::ProgressBar::new(usage_pct / 100.0)
                    .desired_width(ui.available_width())
                    .desired_height(32.0)
                    .fill(self.get_usage_color(usage_pct))
                    .rounding(8.0),
            );

            ui.add_space(12.0);

            // Memory stats
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

            // Memory history chart
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

    fn render_jobs_section(&self, ui: &mut egui::Ui, layout: LayoutMode) {
        self.render_card(ui, |ui| {
            self.render_section_header(ui, "Job Performance", "Latency and success metrics");
            ui.add_space(20.0);

            let jobs = self.metrics.get_snapshot();

            if jobs.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(32.0);
                    ui.label(
                        egui::RichText::new("No active jobs")
                            .size(14.0)
                            .color(self.theme.text_secondary),
                    );
                    ui.add_space(32.0);
                });
                return;
            }

            for (name, stats) in jobs {
                self.render_job_card(ui, &name, &stats, layout);
                ui.add_space(16.0);
            }
        });
    }

    fn render_job_card(&self, ui: &mut egui::Ui, name: &str, stats: &JobStats, layout: LayoutMode) {
        egui::Frame::none()
            .fill(self.theme.bg_secondary)
            .rounding(8.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, self.theme.border))
            .show(ui, |ui| {
                // Job header
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(name)
                            .size(16.0)
                            .strong()
                            .color(self.theme.text_primary),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let success_rate = stats.success_rate();
                        let color = self.get_status_color(success_rate);

                        ui.label(
                            egui::RichText::new(format!("{:.1}%", success_rate))
                                .size(14.0)
                                .strong()
                                .color(color),
                        );
                    });
                });

                ui.add_space(14.0);

                // Metrics pills
                match layout {
                    LayoutMode::Mobile => {
                        ui.vertical(|ui| {
                            self.render_metric_pill(
                                ui,
                                "Avg",
                                format!("{:.0}ms", stats.avg_latency_ms()),
                            );
                            ui.add_space(8.0);
                            self.render_metric_pill(
                                ui,
                                "Current",
                                format!("{:.0}ms", stats.current_latency_ms()),
                            );
                            ui.add_space(8.0);
                            self.render_metric_pill(
                                ui,
                                "Min/Max",
                                format!(
                                    "{:.0}/{:.0}ms",
                                    stats.min_latency_ms(),
                                    stats.max_latency_ms()
                                ),
                            );
                            ui.add_space(8.0);
                            self.render_metric_pill(ui, "Runs", stats.total_runs.to_string());
                        });
                    }
                    _ => {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.spacing_mut().item_spacing.y = 8.0;
                            self.render_metric_pill(
                                ui,
                                "Avg",
                                format!("{:.0}ms", stats.avg_latency_ms()),
                            );
                            self.render_metric_pill(
                                ui,
                                "Current",
                                format!("{:.0}ms", stats.current_latency_ms()),
                            );
                            self.render_metric_pill(
                                ui,
                                "Min/Max",
                                format!(
                                    "{:.0}/{:.0}ms",
                                    stats.min_latency_ms(),
                                    stats.max_latency_ms()
                                ),
                            );
                            self.render_metric_pill(ui, "Runs", stats.total_runs.to_string());
                        });
                    }
                }

                ui.add_space(14.0);

                // Latency chart
                let chart_height = match layout {
                    LayoutMode::Desktop => 90.0,
                    LayoutMode::Tablet => 80.0,
                    LayoutMode::Mobile => 70.0,
                };
                self.render_chart(
                    ui,
                    &stats.latency_history,
                    format!("job_{}", name),
                    chart_height,
                );
            });
    }

    fn render_metric_pill(&self, ui: &mut egui::Ui, label: &str, value: String) {
        egui::Frame::none()
            .fill(self.theme.bg_card)
            .rounding(6.0)
            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(
                        egui::RichText::new(format!("{}:", label))
                            .size(11.0)
                            .color(self.theme.text_secondary),
                    );
                    ui.label(
                        egui::RichText::new(value)
                            .size(12.0)
                            .strong()
                            .color(self.theme.text_primary),
                    );
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

    fn get_status_color(&self, success_rate: f32) -> egui::Color32 {
        if success_rate >= 95.0 {
            self.theme.success
        } else if success_rate >= 80.0 {
            self.theme.warning
        } else {
            self.theme.danger
        }
    }
}

// ================= Main Entry Point =================

#[tokio::main]
async fn main() {
    let metrics = Metrics::default();

    // Spawn background job simulator
    let metrics_clone = metrics.clone();
    tokio::spawn(async move {
        let mut rng = StdRng::from_entropy();

        loop {
            let latency_ms = rng.gen_range(50..500);
            let duration = Duration::from_millis(latency_ms);
            let success = rng.gen_bool(0.85); // 85% success rate

            metrics_clone.record("api_requests", duration, success);
            sleep(Duration::from_millis(500)).await;
        }
    });

    // Launch GUI with optimized settings
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
        Box::new(|_cc| Box::new(SystemMonitor::new(metrics))),
    )
    .expect("Failed to launch application");
}
