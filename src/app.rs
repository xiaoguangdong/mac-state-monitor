use crate::alert::AlertManager;
use crate::config::Config;
use crate::launch_agent;
use crate::model::HistoryBuffer;
use crate::monitor::SystemMonitor;
use crate::ui::chart_window::{ChartMode, ChartWindow};
use crate::ui::tray::TrayManager;
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
        self.config.save();
    }

    pub fn set_temp_component(&mut self, label: String) {
        self.config.menubar_temp_component = label;
        self.config.save();
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
}
