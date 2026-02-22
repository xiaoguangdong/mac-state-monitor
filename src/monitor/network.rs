use crate::model::NetworkStats;
use sysinfo::Networks;

pub fn total_bytes(networks: &Networks) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    for (_name, data) in networks.iter() {
        rx += data.total_received();
        tx += data.total_transmitted();
    }
    (rx, tx)
}

pub fn collect(
    networks: &Networks,
    prev_rx: u64,
    prev_tx: u64,
    elapsed_secs: f64,
) -> (NetworkStats, u64, u64) {
    let (rx, tx) = total_bytes(networks);
    let delta_rx = rx.saturating_sub(prev_rx);
    let delta_tx = tx.saturating_sub(prev_tx);

    let stats = NetworkStats {
        total_received_bytes: rx,
        total_transmitted_bytes: tx,
        received_per_sec: (delta_rx as f64 / elapsed_secs) as u64,
        transmitted_per_sec: (delta_tx as f64 / elapsed_secs) as u64,
    };

    (stats, rx, tx)
}
