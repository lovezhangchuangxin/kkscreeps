//! 当前执行线程的累计 CPU 时间；不包含线程等待或其他线程的运行时间。

use std::io;
use std::time::Duration;

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn thread_cpu_time() -> io::Result<Duration> {
    let mut stamp = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: stamp 是有效可写 timespec，系统调用只写入该结构，不保留指针。
    let status = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut stamp) };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    if stamp.tv_sec < 0 || !(0..1_000_000_000).contains(&stamp.tv_nsec) {
        return Err(io::Error::other("线程 CPU 时钟返回非法时间"));
    }
    Ok(Duration::new(stamp.tv_sec as u64, stamp.tv_nsec as u32))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn thread_cpu_time() -> io::Result<Duration> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "当前平台未实现线程 CPU 时钟",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sleep_and_other_thread_work_are_not_billed() -> io::Result<()> {
        let before = thread_cpu_time()?;
        let worker = std::thread::spawn(|| {
            let start = std::time::Instant::now();
            let mut value = 1u64;
            while start.elapsed() < Duration::from_millis(80) {
                value = std::hint::black_box(value.wrapping_mul(3).wrapping_add(1));
            }
            value
        });
        std::thread::sleep(Duration::from_millis(80));
        worker.join().unwrap();
        let elapsed = thread_cpu_time()?.saturating_sub(before);
        assert!(
            elapsed < Duration::from_millis(40),
            "等待不应计成 CPU 用量：{elapsed:?}"
        );
        let before = thread_cpu_time()?;
        let mut value = 1u64;
        while thread_cpu_time()?.saturating_sub(before) < Duration::from_millis(10) {
            value = std::hint::black_box(value.wrapping_mul(3).wrapping_add(1));
        }
        assert!(thread_cpu_time()? >= before + Duration::from_millis(10));
        std::hint::black_box(value);
        Ok(())
    }
}
