use crate::model::DiskStats;
use std::collections::HashSet;
use sysinfo::Disks;

pub fn collect(disks: &Disks) -> Vec<DiskStats> {
    let mut seen_names = HashSet::new();
    disks
        .iter()
        .filter(|d| {
            let mp = d.mount_point().to_string_lossy().to_string();
            let name = d.name().to_string_lossy().to_string();
            // Skip zero-size, /System/Volumes mounts, and duplicate disk names
            d.total_space() > 0 && !mp.starts_with("/System/Volumes") && seen_names.insert(name)
        })
        .map(|d| {
            let total = d.total_space();
            let available = d.available_space();
            let used = total.saturating_sub(available);
            let usage_percent = if total > 0 {
                (used as f32 / total as f32) * 100.0
            } else {
                0.0
            };
            DiskStats {
                name: d.name().to_string_lossy().to_string(),
                mount_point: d.mount_point().to_string_lossy().to_string(),
                total_bytes: total,
                available_bytes: available,
                usage_percent,
            }
        })
        .collect()
}
