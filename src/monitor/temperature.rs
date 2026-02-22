use crate::model::{TemperatureReading, TemperatureStats};
use sysinfo::Components;

pub fn collect_from(components: &Components) -> TemperatureStats {
    let mut cpu_temps = Vec::new();
    let mut gpu_temps = Vec::new();
    let mut ssd_temp: Option<f32> = None;
    let mut other: Vec<(String, f32)> = Vec::new();

    for comp in components {
        let temp = match comp.temperature() {
            Some(t) => t,
            None => continue,
        };
        // Skip invalid temperature readings
        if temp <= 0.0 || temp > 150.0 {
            continue;
        }
        let label = comp.label();
        let lower = label.to_lowercase();

        if lower.contains("nand") || lower.contains("ssd") || lower.contains("disk") {
            ssd_temp = Some(temp);
        } else if lower.starts_with("pmu tdie") {
            // Apple Silicon: PMU tdie* = CPU performance cores
            cpu_temps.push(temp);
        } else if lower.starts_with("pmu2 tdie") {
            // Apple Silicon: PMU2 tdie* = GPU cores
            gpu_temps.push(temp);
        } else if lower.starts_with("pmu tdev") {
            // CPU efficiency/device temps, use as CPU fallback
            cpu_temps.push(temp);
        } else if lower.contains("cpu") {
            cpu_temps.push(temp);
        } else if lower.contains("gpu") {
            gpu_temps.push(temp);
        } else if !lower.contains("pmu") {
            // Skip other PMU entries, keep truly different sensors
            other.push((label.to_string(), temp));
        }
    }

    let mut readings = Vec::new();

    if !cpu_temps.is_empty() {
        let avg = cpu_temps.iter().sum::<f32>() / cpu_temps.len() as f32;
        readings.push(TemperatureReading {
            label: "CPU".to_string(),
            temp_c: avg,
        });
    }

    if !gpu_temps.is_empty() {
        let avg = gpu_temps.iter().sum::<f32>() / gpu_temps.len() as f32;
        readings.push(TemperatureReading {
            label: "GPU".to_string(),
            temp_c: avg,
        });
    }

    if let Some(t) = ssd_temp {
        readings.push(TemperatureReading {
            label: "SSD".to_string(),
            temp_c: t,
        });
    }

    for (label, temp) in other {
        readings.push(TemperatureReading { label, temp_c: temp });
    }

    // Only fallback to powermetrics if no readings and we're confident it will work
    // Avoid calling powermetrics on every tick as it requires privileges
    if readings.is_empty() {
        // Return empty readings instead of calling powermetrics which may cause issues
        return TemperatureStats::default();
    }

    TemperatureStats { readings }
}
