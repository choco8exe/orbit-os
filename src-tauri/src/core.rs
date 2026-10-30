use chrono::Utc;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;
use sysinfo::{Disks, Networks, System};

pub struct CoreState {
    system: Mutex<System>,
    disks: Mutex<Disks>,
    networks: Mutex<Networks>,
    network_sample: Mutex<Option<(u64, u64, Instant)>>,
    has_cpu_sample: Mutex<bool>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            system: Mutex::new(System::new_all()),
            disks: Mutex::new(Disks::new_with_refreshed_list()),
            networks: Mutex::new(Networks::new_with_refreshed_list()),
            network_sample: Mutex::new(None),
            has_cpu_sample: Mutex::new(false),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Metric<T: Serialize> {
    value: Option<T>,
    unit: String,
    availability: &'static str,
    detail: Option<String>,
    sampled_at: Option<String>,
}

fn measured<T: Serialize>(value: T, unit: &str, sampled_at: &str) -> Metric<T> {
    Metric {
        value: Some(value),
        unit: unit.into(),
        availability: "available",
        detail: None,
        sampled_at: Some(sampled_at.into()),
    }
}

fn unavailable<T: Serialize>(unit: &str, reason: &str) -> Metric<T> {
    Metric {
        value: None,
        unit: unit.into(),
        availability: "unavailable",
        detail: Some(reason.into()),
        sampled_at: None,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CpuMetrics {
    name: Metric<String>,
    usage_percent: Metric<f32>,
    cores: Metric<usize>,
    threads: Metric<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryMetrics {
    total_bytes: Metric<u64>,
    used_bytes: Metric<u64>,
    available_bytes: Metric<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiskMetric {
    mount: String,
    total_bytes: Metric<u64>,
    free_bytes: Metric<u64>,
    used_percent: Metric<f32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkMetrics {
    received_bytes_per_second: Metric<u64>,
    sent_bytes_per_second: Metric<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GpuMetrics {
    name: Metric<String>,
    usage_percent: Metric<f32>,
    memory_used_bytes: Metric<u64>,
    memory_total_bytes: Metric<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    captured_at: String,
    cpu: CpuMetrics,
    memory: MemoryMetrics,
    disks: Vec<DiskMetric>,
    network: NetworkMetrics,
    gpu: GpuMetrics,
    uptime_seconds: Metric<u64>,
}

#[tauri::command]
pub fn get_system_snapshot(state: tauri::State<'_, CoreState>) -> Snapshot {
    let mut system = state
        .system
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    system.refresh_memory();
    system.refresh_cpu_usage();
    let mut has_cpu_sample = state
        .has_cpu_sample
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cpu_ready = *has_cpu_sample;
    *has_cpu_sample = true;
    let sampled_at = Utc::now().to_rfc3339();
    let cpu_name = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_owned())
        .filter(|s| !s.is_empty());
    let thread_count = system.cpus().len();
    let core_count = system.physical_core_count().unwrap_or(thread_count);
    let total_memory = system.total_memory();
    let used_memory = system.used_memory();
    let mut disk_state = state
        .disks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    disk_state.refresh(true);
    let disks = disk_state
        .iter()
        .map(|disk| {
            let total = disk.total_space();
            let free = disk.available_space();
            let used_percent = if total > 0 {
                ((total.saturating_sub(free) as f64 / total as f64) * 100.0) as f32
            } else {
                0.0
            };
            DiskMetric {
                mount: disk.mount_point().to_string_lossy().to_string(),
                total_bytes: measured(total, "bytes", &sampled_at),
                free_bytes: measured(free, "bytes", &sampled_at),
                used_percent: measured(used_percent, "%", &sampled_at),
            }
        })
        .collect();
    let mut network_state = state
        .networks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    network_state.refresh(true);
    let received: u64 = network_state
        .iter()
        .map(|(_, data)| data.total_received())
        .sum();
    let sent: u64 = network_state
        .iter()
        .map(|(_, data)| data.total_transmitted())
        .sum();
    drop(network_state);
    let now = Instant::now();
    let mut previous = state
        .network_sample
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let rates = previous.take().map(|(old_received, old_sent, old_at)| {
        let elapsed = now.duration_since(old_at).as_secs_f64().max(0.001);
        (
            (received.saturating_sub(old_received) as f64 / elapsed) as u64,
            (sent.saturating_sub(old_sent) as f64 / elapsed) as u64,
        )
    });
    *previous = Some((received, sent, now));
    drop(previous);

    Snapshot {
        captured_at: sampled_at.clone(),
        cpu: CpuMetrics {
            name: match cpu_name {
                Some(value) => measured(value, "", &sampled_at),
                None => unavailable("", "Le système ne fournit pas le nom du processeur."),
            },
            usage_percent: if cpu_ready {
                measured(system.global_cpu_usage(), "%", &sampled_at)
            } else {
                unavailable("%", "En attente du second échantillon CPU.")
            },
            cores: measured(core_count, "", &sampled_at),
            threads: measured(thread_count, "", &sampled_at),
        },
        memory: MemoryMetrics {
            total_bytes: measured(total_memory, "bytes", &sampled_at),
            used_bytes: measured(used_memory, "bytes", &sampled_at),
            available_bytes: measured(system.available_memory(), "bytes", &sampled_at),
        },
        disks,
        network: NetworkMetrics {
            received_bytes_per_second: match rates {
                Some((received, _)) => measured(received, "bytes/s", &sampled_at),
                None => unavailable("bytes/s", "En attente du second échantillon réseau."),
            },
            sent_bytes_per_second: match rates {
                Some((_, sent)) => measured(sent, "bytes/s", &sampled_at),
                None => unavailable("bytes/s", "En attente du second échantillon réseau."),
            },
        },
        gpu: GpuMetrics {
            name: unavailable("", "Le pilote ne fournit pas encore une source GPU fiable."),
            usage_percent: unavailable(
                "%",
                "Le pilote ne fournit pas encore une source GPU fiable.",
            ),
            memory_used_bytes: unavailable(
                "bytes",
                "Le pilote ne fournit pas encore une source GPU fiable.",
            ),
            memory_total_bytes: unavailable(
                "bytes",
                "Le pilote ne fournit pas encore une source GPU fiable.",
            ),
        },
        uptime_seconds: measured(System::uptime(), "s", &sampled_at),
    }
}
