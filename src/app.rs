use crate::alert::AlertManager;
use crate::config::Config;
use crate::launch_agent;
use crate::model::HistoryBuffer;
use crate::monitor::SystemMonitor;
use crate::ui::chart_window::{ChartMode, ChartWindow};
use crate::ui::tray::TrayManager;
use std::time::Instant;
use tao::event_loop::EventLoopWindowTarget;

pub struct App {
    config: Config,
    monitor: SystemMonitor,
    tray: TrayManager,
    alert: AlertManager,
    pub history: HistoryBuffer,
    pub chart_window: ChartWindow,
}

impl App {
    pub fn new() -> Self {
        let mut config = Config::load();
        config.launch_at_login = launch_agent::is_enabled();
        Self {
            config,
            monitor: SystemMonitor::new(),
            tray: TrayManager::new(),
            alert: AlertManager::new(),
            history: HistoryBuffer::new(60),
            chart_window: ChartWindow::new(),
        }
    }

    pub fn tick(&mut self) {
        let stats = self.monitor.poll();
        self.history.push(&stats);
        self.tray.update(&stats, &self.config);
        self.alert.check(&stats);
        self.chart_window.render(&self.history);
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn set_poll_interval(&mut self, secs: u64) {
        self.config.poll_interval_secs = secs;
        self.tray.invalidate_cpu_menu();
        self.config.save();
    }

    pub fn set_temp_component(&mut self, label: String) {
        self.config.menubar_temp_component = label;
        self.tray.invalidate_cpu_menu();
        self.config.save();
    }

    pub fn set_runner_display_secs(&mut self, secs: u64) {
        self.config.runner_display_secs = secs.clamp(1, 3600);
        self.tray.sync_runner_config(&self.config);
        self.config.save();
    }

    pub fn toggle_runner_in_rotation(&mut self, runner_id: String) {
        if self
            .tray
            .toggle_runner_in_rotation(&mut self.config, &runner_id)
        {
            self.config.save();
        }
    }

    pub fn select_all_runners(&mut self) {
        self.tray.select_all_runners(&mut self.config);
        self.config.save();
    }

    pub fn select_runner_category(&mut self, category: String) {
        self.tray.select_runner_category(&mut self.config, &category);
        self.config.save();
    }

    pub fn import_custom_runner(&mut self) {
        if self.tray.import_custom_runner_frames(&mut self.config) {
            self.config.save();
        }
    }

    pub fn toggle_launch_at_login(&mut self) {
        self.config.launch_at_login = !self.config.launch_at_login;
        launch_agent::set_enabled(self.config.launch_at_login);
        self.config.save();
    }

    pub fn toggle_charts(&mut self, event_loop: &EventLoopWindowTarget<()>, mode: ChartMode) {
        self.chart_window.toggle(event_loop, mode);
        if self.chart_window.is_visible() {
            self.chart_window.render(&self.history);
        }
    }

    pub fn animate(&mut self, now: Instant) {
        self.tray.animate(now);
    }
}
