mod cpu;
mod disk;
mod memory;
mod network;
pub mod temperature;

use crate::model::*;
use std::time::Instant;
use sysinfo::{Components, Disks, Networks, System};

pub struct SystemMonitor {
    sys: System,
    networks: Networks,
    disks: Disks,
    components: Components,
    last_poll: Instant,
    prev_net_rx: u64,
    prev_net_tx: u64,
}

impl SystemMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_cpu_usage();
        let networks = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();
        let components = Components::new_with_refreshed_list();

        let (rx, tx) = network::total_bytes(&networks);

        Self {
            sys,
            networks,
            disks,
            components,
            last_poll: Instant::now(),
            prev_net_rx: rx,
            prev_net_tx: tx,
        }
    }

    pub fn poll(&mut self) -> SystemStats {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_poll).as_secs_f64().max(0.1);

        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.disks.refresh(true);
        self.networks.refresh(true);
        self.components.refresh(true);

        let cpu = cpu::collect(&self.sys);
        let memory = memory::collect(&self.sys);
        let disks = disk::collect(&self.disks);
        let (net, new_rx, new_tx) =
            network::collect(&self.networks, self.prev_net_rx, self.prev_net_tx, elapsed);

        self.prev_net_rx = new_rx;
        self.prev_net_tx = new_tx;
        self.last_poll = now;

        let temp = temperature::collect_from(&self.components);

        SystemStats {
            cpu,
            memory,
            disks,
            network: net,
            temperature: temp,
            timestamp: now,
        }
    }
}
