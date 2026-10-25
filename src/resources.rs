use serde::{Deserialize, Serialize};

/// Snapshot of resources available to this process, refreshed at each assignment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resources {
    pub cpus: usize,
    pub total_memory: u64,
    pub available_memory: u64,
}

impl Resources {
    pub fn detect() -> Self {
        let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
        let (total_memory, available_memory) = memory();
        Self { cpus, total_memory, available_memory }
    }

    pub fn threads(&self, width: u32, height: u32) -> usize {
        if self.available_memory == 0 {
            return self.cpus.clamp(1, 256);
        }
        // Conservative guard against allocating many full-size intermediate surfaces
        // on a shared-memory laptop. This is a budget estimate, not a VRAM measurement.
        let per_thread = (u64::from(width) * u64::from(height)).saturating_mul(32).max(128 * 1024 * 1024);
        let usable = self.available_memory.saturating_sub(1024 * 1024 * 1024);
        (usable / per_thread).max(1).min(self.cpus.clamp(1, 256) as u64) as usize
    }

    pub fn summary(&self, threads: usize, gpu: bool) -> String {
        let memory = if self.total_memory == 0 {
            "RAM 정보 없음".to_owned()
        } else {
            format!("RAM 여유 {:.1}/{:.1} GiB", self.available_memory as f64 / 1073741824.0, self.total_memory as f64 / 1073741824.0)
        };
        format!("CPU {}개 · 작업 스레드 {threads} · {memory} · 렌더 {}", self.cpus, if gpu { "GPU" } else { "CPU" })
    }
}

#[cfg(windows)]
fn memory() -> (u64, u64) {
    #[repr(C)]
    struct MemoryStatus {
        length: u32,
        load: u32,
        total: u64,
        available: u64,
        page_total: u64,
        page_available: u64,
        virtual_total: u64,
        virtual_available: u64,
        extended: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(status: *mut MemoryStatus) -> i32;
    }
    let mut status = MemoryStatus {
        length: std::mem::size_of::<MemoryStatus>() as u32,
        load: 0,
        total: 0,
        available: 0,
        page_total: 0,
        page_available: 0,
        virtual_total: 0,
        virtual_available: 0,
        extended: 0,
    };
    // SAFETY: correctly sized and aligned MEMORYSTATUSEX remains alive for the call.
    if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 { (status.total, status.available) } else { (0, 0) }
}

#[cfg(not(windows))]
fn memory() -> (u64, u64) {
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_pressure_reduces_parallelism_without_zero_threads() {
        let mut resources = Resources { cpus: 16, total_memory: 8 << 30, available_memory: 6 << 30 };
        assert_eq!(resources.threads(1920, 1080), 16);
        resources.available_memory = 2 << 30;
        assert_eq!(resources.threads(3840, 2160), 4);
        resources.available_memory = 512 << 20;
        assert_eq!(resources.threads(3840, 2160), 1);
        resources.available_memory = 0;
        assert_eq!(resources.threads(3840, 2160), 16);
    }
}
