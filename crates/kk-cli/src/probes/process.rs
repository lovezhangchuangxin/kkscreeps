//! 测量当前进程全部线程的 CPU 与采样点 RSS，不参与玩家 CPU 计费。

use std::time::Instant;

use serde_json::{Value, json};

const MAX_RSS_SAMPLES: usize = 50_000;

pub(crate) struct ProcessUsage {
    pub cpu_core_ms: f64,
    pub voluntary_context_switches: u64,
    pub involuntary_context_switches: u64,
}

pub(crate) struct ProcessProbe {
    started: Instant,
    cpu_before: f64,
    rss: Vec<u64>,
    rss_peak: u64,
    observations: u64,
    sampling_ms: f64,
}

impl ProcessProbe {
    pub(crate) fn start() -> anyhow::Result<Self> {
        let rss = resident_bytes()?;
        Ok(Self {
            started: Instant::now(),
            cpu_before: cpu_ms()?,
            rss: vec![rss],
            rss_peak: rss,
            observations: 1,
            sampling_ms: 0.0,
        })
    }

    pub(crate) fn observe(&mut self) -> anyhow::Result<()> {
        let t0 = Instant::now();
        let rss = resident_bytes()?;
        self.observations += 1;
        self.rss_peak = self.rss_peak.max(rss);
        if self.rss.len() < MAX_RSS_SAMPLES {
            self.rss.push(rss);
        }
        self.sampling_ms += t0.elapsed().as_secs_f64() * 1000.0;
        Ok(())
    }

    pub(crate) fn finish(mut self, ticks: u32) -> anyhow::Result<Value> {
        let cpu = (cpu_ms()? - self.cpu_before).max(0.0);
        let wall = self.started.elapsed().as_secs_f64() * 1000.0;
        let rss_after = resident_bytes()?;
        self.rss.sort_unstable();
        Ok(json!({
            "scope": "current_process_all_threads",
            "window": "after_warmup_through_collection_loop",
            "wall_ms": wall,
            "cpu_core_ms": cpu,
            "cpu_core_ms_per_tick": cpu / f64::from(ticks),
            "rss_bytes_after": rss_after,
            "rss_bytes_sampled_peak": self.rss_peak,
            "rss_bytes_sampled_p50": self.rss[((self.rss.len() - 1) as f64 * 0.5).round() as usize],
            "rss_observations": self.observations,
            "rss_retained_samples": self.rss.len(),
            "rss_sampling_wall_ms": self.sampling_ms,
            "includes_sampling_and_workload_checks": true,
            "rss_sampling": "before_collection_and_after_each_tick; p50_uses_first_50000_observations",
        }))
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn process_usage() -> anyhow::Result<ProcessUsage> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // getrusage 填满有效输出缓冲区；只在返回成功后读取。
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let usage = unsafe { usage.assume_init() };
    let ms = |t: libc::timeval| t.tv_sec as f64 * 1000.0 + t.tv_usec as f64 / 1000.0;
    Ok(ProcessUsage {
        cpu_core_ms: ms(usage.ru_utime) + ms(usage.ru_stime),
        voluntary_context_switches: usage.ru_nvcsw.try_into()?,
        involuntary_context_switches: usage.ru_nivcsw.try_into()?,
    })
}

fn cpu_ms() -> anyhow::Result<f64> {
    Ok(process_usage()?.cpu_core_ms)
}

#[cfg(target_os = "macos")]
pub(super) fn resident_bytes() -> anyhow::Result<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v2>::uninit();
    // libproc 按 flavor 将 rusage_info_v2 写入 buffer 指向的存储；
    // rusage_info_t* 是其 C 接口的通用缓冲区类型，不是另分配的输出指针。
    let rc = unsafe {
        libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V2,
            usage.as_mut_ptr().cast(),
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { usage.assume_init() }.ri_resident_size)
}

#[cfg(target_os = "linux")]
pub(super) fn resident_bytes() -> anyhow::Result<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm")?;
    let pages: u64 = statm
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("/proc/self/statm 缺少 resident 字段"))?
        .parse()?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    anyhow::ensure!(page_size > 0, "无法读取系统页大小");
    pages
        .checked_mul(page_size as u64)
        .ok_or_else(|| anyhow::anyhow!("RSS 字节数溢出"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn process_usage() -> anyhow::Result<ProcessUsage> {
    anyhow::bail!("进程资源采样目前仅支持 macOS/Linux")
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(super) fn resident_bytes() -> anyhow::Result<u64> {
    anyhow::bail!("进程资源采样目前仅支持 macOS/Linux")
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn cpu_includes_worker_thread_and_rss_is_bytes() -> anyhow::Result<()> {
        let before = cpu_ms()?;
        std::thread::spawn(|| {
            let t0 = Instant::now();
            let mut n = 1u64;
            while t0.elapsed().as_millis() < 30 {
                n = std::hint::black_box(n.wrapping_mul(6364136223846793005).wrapping_add(1));
            }
            n
        })
        .join()
        .unwrap();
        assert!(cpu_ms()? > before, "CPU 应包含工作线程用量");
        assert!(resident_bytes()? > 4096, "RSS 应为字节数而非页数");
        Ok(())
    }
}
