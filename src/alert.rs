use crate::model::SystemStats;
use std::process::Command;
use std::time::Instant;

const COOLDOWN_SECS: u64 = 60;

pub struct AlertManager {
    cpu_threshold: f32,
    mem_threshold: f32,
    temp_threshold: f32,
    last_cpu_alert: Option<Instant>,
    last_mem_alert: Option<Instant>,
    last_temp_alert: Option<Instant>,
}

impl AlertManager {
    pub fn new() -> Self {
        Self {
            cpu_threshold: 90.0,
            mem_threshold: 90.0,
            temp_threshold: 95.0,
            last_cpu_alert: None,
            last_mem_alert: None,
            last_temp_alert: None,
        }
    }

    pub fn check(&mut self, stats: &SystemStats) {
        let now = Instant::now();

        if stats.cpu.global_usage >= self.cpu_threshold && self.can_alert(&self.last_cpu_alert, now)
        {
            notify(
                "CPU Usage High",
                &format!("CPU at {:.0}%", stats.cpu.global_usage),
            );
            self.last_cpu_alert = Some(now);
        }

        if stats.memory.usage_percent >= self.mem_threshold
            && self.can_alert(&self.last_mem_alert, now)
        {
            notify(
                "Memory Usage High",
                &format!("Memory at {:.0}%", stats.memory.usage_percent),
            );
            self.last_mem_alert = Some(now);
        }

        let max_temp = stats
            .temperature
            .readings
            .iter()
            .map(|r| r.temp_c)
            .fold(0.0_f32, f32::max);
        if max_temp >= self.temp_threshold && self.can_alert(&self.last_temp_alert, now) {
            notify(
                "Temperature High",
                &format!("Temperature at {:.0}C", max_temp),
            );
            self.last_temp_alert = Some(now);
        }
    }

    fn can_alert(&self, last: &Option<Instant>, now: Instant) -> bool {
        match last {
            None => true,
            Some(t) => now.duration_since(*t).as_secs() >= COOLDOWN_SECS,
        }
    }
}

fn notify(title: &str, message: &str) {
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        message, title
    );
    let _ = Command::new("osascript").arg("-e").arg(&script).spawn();
}
