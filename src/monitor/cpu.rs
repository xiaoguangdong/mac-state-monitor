use crate::model::CpuStats;
use sysinfo::System;

pub fn collect(sys: &System) -> CpuStats {
    let cpus = sys.cpus();
    CpuStats {
        global_usage: sys.global_cpu_usage(),
        per_core_usage: cpus.iter().map(|c| c.cpu_usage()).collect(),
        core_count: cpus.len(),
    }
}
