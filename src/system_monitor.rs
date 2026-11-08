use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct SystemMetrics {
    pub cpu_pct: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub ram_pct: f32,
    pub net_rx_kbs: f64,
    pub net_tx_kbs: f64,
    pub scan_pps: u32,
}

pub struct SysMonitor {
    last_cpu_total: u64,
    last_cpu_idle: u64,
    last_net_rx: u64,
    last_net_tx: u64,
    last_scanned: u32,
    last_update: Instant,
    metrics: SystemMetrics,
}

impl SysMonitor {
    pub fn new() -> Self {
        let (cpu_total, cpu_idle) = Self::read_cpu_raw();
        let (net_rx, net_tx) = Self::read_net_raw();

        Self {
            last_cpu_total: cpu_total,
            last_cpu_idle: cpu_idle,
            last_net_rx: net_rx,
            last_net_tx: net_tx,
            last_scanned: 0,
            last_update: Instant::now(),
            metrics: SystemMetrics::default(),
        }
    }

    pub fn update(&mut self, scanned_ips: &Arc<AtomicU32>) -> SystemMetrics {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f64();
        if elapsed < 0.5 {
            return self.metrics.clone();
        }

        self.last_update = now;

        // 1. CPU Usage Calculation
        let (cpu_total, cpu_idle) = Self::read_cpu_raw();
        let diff_total = cpu_total.saturating_sub(self.last_cpu_total);
        let diff_idle = cpu_idle.saturating_sub(self.last_cpu_idle);

        if diff_total > 0 {
            let active = diff_total.saturating_sub(diff_idle);
            self.metrics.cpu_pct = ((active as f64 / diff_total as f64) * 100.0) as f32;
        }
        self.last_cpu_total = cpu_total;
        self.last_cpu_idle = cpu_idle;

        // 2. RAM Usage Calculation
        let (used_mb, total_mb) = Self::read_ram_raw();
        self.metrics.ram_used_mb = used_mb;
        self.metrics.ram_total_mb = total_mb;
        if total_mb > 0 {
            self.metrics.ram_pct = ((used_mb as f64 / total_mb as f64) * 100.0) as f32;
        }

        // 3. Network Bandwidth Calculation (RX / TX)
        let (net_rx, net_tx) = Self::read_net_raw();
        let diff_rx = net_rx.saturating_sub(self.last_net_rx);
        let diff_tx = net_tx.saturating_sub(self.last_net_tx);

        if elapsed > 0.0 {
            self.metrics.net_rx_kbs = (diff_rx as f64 / 1024.0) / elapsed;
            self.metrics.net_tx_kbs = (diff_tx as f64 / 1024.0) / elapsed;
        }
        self.last_net_rx = net_rx;
        self.last_net_tx = net_tx;

        // 4. Scan Packets/IPs Per Second (PPS)
        let current_scanned = scanned_ips.load(Ordering::Relaxed);
        let diff_scanned = current_scanned.saturating_sub(self.last_scanned);
        if elapsed > 0.0 {
            self.metrics.scan_pps = (diff_scanned as f64 / elapsed) as u32;
        }
        self.last_scanned = current_scanned;

        self.metrics.clone()
    }

    fn read_cpu_raw() -> (u64, u64) {
        if let Ok(content) = std::fs::read_to_string("/proc/stat") {
            if let Some(line) = content.lines().next() {
                if line.starts_with("cpu ") {
                    let parts: Vec<u64> = line[4..]
                        .split_whitespace()
                        .filter_map(|s| s.parse::<u64>().ok())
                        .collect();
                    if parts.len() >= 4 {
                        let idle = parts[3] + parts.get(4).copied().unwrap_or(0);
                        let total: u64 = parts.iter().sum();
                        return (total, idle);
                    }
                }
            }
        }
        (0, 0)
    }

    fn read_ram_raw() -> (u64, u64) {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            let mut total_kb = 0u64;
            let mut avail_kb = 0u64;
            let mut free_kb = 0u64;
            let mut buffers_kb = 0u64;
            let mut cached_kb = 0u64;

            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    total_kb = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                } else if line.starts_with("MemAvailable:") {
                    avail_kb = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                } else if line.starts_with("MemFree:") {
                    free_kb = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                } else if line.starts_with("Buffers:") {
                    buffers_kb = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                } else if line.starts_with("Cached:") {
                    cached_kb = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                }
            }

            if avail_kb == 0 {
                avail_kb = free_kb + buffers_kb + cached_kb;
            }

            let used_kb = total_kb.saturating_sub(avail_kb);
            return (used_kb / 1024, total_kb / 1024);
        }
        (0, 0)
    }

    fn read_net_raw() -> (u64, u64) {
        if let Ok(content) = std::fs::read_to_string("/proc/net/dev") {
            let mut total_rx = 0u64;
            let mut total_tx = 0u64;

            for line in content.lines().skip(2) {
                if let Some((iface, data)) = line.split_once(':') {
                    if iface.trim() == "lo" {
                        continue;
                    }
                    let fields: Vec<u64> = data
                        .split_whitespace()
                        .filter_map(|s| s.parse::<u64>().ok())
                        .collect();
                    if fields.len() >= 9 {
                        total_rx += fields[0]; // rx bytes
                        total_tx += fields[8]; // tx bytes
                    }
                }
            }
            return (total_rx, total_tx);
        }
        (0, 0)
    }
}
