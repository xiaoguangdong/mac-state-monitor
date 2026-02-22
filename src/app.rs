use crate::config::Config;
use crate::model::HistoryBuffer;
use crate::monitor::SystemMonitor;
use crate::ui::chart_window::ChartWindow;
use crate::ui::tray::TrayManager;
use tao::event_loop::EventLoopWindowTarget;

pub struct App {
    config: Config,
    monitor: SystemMonitor,
    tray: TrayManager,
    pub history: HistoryBuffer,
    pub chart_window: ChartWindow,
}

impl App {
    pub fn new() -> Self {
        Self {
            config: Config::default(),
            monitor: SystemMonitor::new(),
            tray: TrayManager::new(),
            history: HistoryBuffer::new(60),
            chart_window: ChartWindow::new(),
        }
    }

    pub fn tick(&mut self) {
        let stats = self.monitor.poll();
        self.history.push(&stats.temperature);
        self.tray.update(&stats, &self.config);
        self.chart_window.render(&self.history);
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn set_poll_interval(&mut self, secs: u64) {
        self.config.poll_interval_secs = secs;
    }

    pub fn set_temp_component(&mut self, label: String) {
        self.config.menubar_temp_component = label;
    }

    pub fn toggle_charts(&mut self, event_loop: &EventLoopWindowTarget<()>) {
        self.chart_window.toggle(event_loop);
        if self.chart_window.is_visible() {
            self.chart_window.render(&self.history);
        }
    }
}
