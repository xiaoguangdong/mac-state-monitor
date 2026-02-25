use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::time::Instant;

pub struct SystemStats {
    pub cpu: CpuStats,
    pub memory: MemoryStats,
    pub disks: Vec<DiskStats>,
    pub network: NetworkStats,
    pub temperature: TemperatureStats,
    pub timestamp: Instant,
}

#[derive(Clone, Debug)]
pub struct TemperatureReading {
    pub label: String,
    pub temp_c: f32,
}

#[derive(Clone, Default)]
pub struct TemperatureStats {
    pub readings: Vec<TemperatureReading>,
}

impl TemperatureStats {
    pub fn find_temp(&self, label_fragment: &str) -> Option<f32> {
        let frag = label_fragment.to_lowercase();
        self.readings
            .iter()
            .find(|r| r.label.to_lowercase().contains(&frag))
            .map(|r| r.temp_c)
    }
}

pub struct HistoryBuffer {
    pub temps: BTreeMap<String, VecDeque<f32>>,
    pub cpu_usage: VecDeque<f32>,
    pub mem_usage: VecDeque<f32>,
    pub net_down: VecDeque<f64>,
    pub net_up: VecDeque<f64>,
    pub max_points: usize,
}

impl HistoryBuffer {
    pub fn new(max_points: usize) -> Self {
        Self {
            temps: BTreeMap::new(),
            cpu_usage: VecDeque::with_capacity(max_points),
            mem_usage: VecDeque::with_capacity(max_points),
            net_down: VecDeque::with_capacity(max_points),
            net_up: VecDeque::with_capacity(max_points),
            max_points,
        }
    }

    fn push_val_f32(buf: &mut VecDeque<f32>, val: f32, max: usize) {
        if buf.len() >= max {
            buf.pop_front();
        }
        buf.push_back(val);
    }

    fn push_val_f64(buf: &mut VecDeque<f64>, val: f64, max: usize) {
        if buf.len() >= max {
            buf.pop_front();
        }
        buf.push_back(val);
    }

    pub fn push(&mut self, stats: &super::model::SystemStats) {
        // Temperatures
        for reading in &stats.temperature.readings {
            let buf = self
                .temps
                .entry(reading.label.clone())
                .or_insert_with(|| VecDeque::with_capacity(self.max_points));
            if buf.len() >= self.max_points {
                buf.pop_front();
            }
            buf.push_back(reading.temp_c);
        }

        // CPU
        Self::push_val_f32(&mut self.cpu_usage, stats.cpu.global_usage, self.max_points);

        // Memory
        Self::push_val_f32(
            &mut self.mem_usage,
            stats.memory.usage_percent,
            self.max_points,
        );

        // Network (convert to KB/s for readability)
        let down_kb = stats.network.received_per_sec as f64 / 1024.0;
        let up_kb = stats.network.transmitted_per_sec as f64 / 1024.0;
        Self::push_val_f64(&mut self.net_down, down_kb, self.max_points);
        Self::push_val_f64(&mut self.net_up, up_kb, self.max_points);
    }
}

pub struct CpuStats {
    pub global_usage: f32,
    pub per_core_usage: Vec<f32>,
    pub core_count: usize,
}

pub struct MemoryStats {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub usage_percent: f32,
}

pub struct DiskStats {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f32,
}

pub struct NetworkStats {
    pub total_received_bytes: u64,
    pub total_transmitted_bytes: u64,
    pub received_per_sec: u64,
    pub transmitted_per_sec: u64,
}

impl Default for SystemStats {
    fn default() -> Self {
        Self {
            cpu: CpuStats {
                global_usage: 0.0,
                per_core_usage: vec![],
                core_count: 0,
            },
            memory: MemoryStats {
                total_bytes: 0,
                used_bytes: 0,
                available_bytes: 0,
                swap_total_bytes: 0,
                swap_used_bytes: 0,
                usage_percent: 0.0,
            },
            disks: vec![],
            network: NetworkStats {
                total_received_bytes: 0,
                total_transmitted_bytes: 0,
                received_per_sec: 0,
                transmitted_per_sec: 0,
            },
            temperature: TemperatureStats::default(),
            timestamp: Instant::now(),
        }
    }
}
