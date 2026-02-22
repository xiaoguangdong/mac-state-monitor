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
    pub max_points: usize,
}

impl HistoryBuffer {
    pub fn new(max_points: usize) -> Self {
        Self {
            temps: BTreeMap::new(),
            max_points,
        }
    }

    pub fn push(&mut self, temp: &TemperatureStats) {
        for reading in &temp.readings {
            let buf = self
                .temps
                .entry(reading.label.clone())
                .or_insert_with(|| VecDeque::with_capacity(self.max_points));
            if buf.len() >= self.max_points {
                buf.pop_front();
            }
            buf.push_back(reading.temp_c);
        }
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
