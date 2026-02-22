use crate::model::MemoryStats;
use sysinfo::System;

pub fn collect(sys: &System) -> MemoryStats {
    let total = sys.total_memory();
    let used = sys.used_memory();
    let available = sys.available_memory();
    let usage_percent = if total > 0 {
        (used as f32 / total as f32) * 100.0
    } else {
        0.0
    };

    MemoryStats {
        total_bytes: total,
        used_bytes: used,
        available_bytes: available,
        swap_total_bytes: sys.total_swap(),
        swap_used_bytes: sys.used_swap(),
        usage_percent,
    }
}
