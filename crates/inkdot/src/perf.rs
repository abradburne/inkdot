//! Asking the scheduler to run the emulation thread's work at a decent clock.
//!
//! The main loop sleeps until just before each vblank, so the CPU looks lightly loaded, and
//! schedutil picks the lowest clock that keeps it about 80% busy: measured on the DS+, a frame
//! that takes ~3 ms at 1.99 GHz was stretched to ~12 ms at ~600 MHz, which delays every button
//! press and misses vblanks when a frame runs long. A utilization clamp (`sched_util_min`, the
//! kernel's uclamp) tells schedutil this thread needs at least that much capacity while it runs:
//! the frame finishes quickly and the core sleeps for the rest. It affects only this thread and
//! needs no privileges. Kernels without CONFIG_UCLAMP_TASK refuse it, and inkdot carries on.

/// struct sched_attr (include/uapi/linux/sched/types.h).
#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Default)]
struct SchedAttr {
    size: u32,
    policy: u32,
    flags: u64,
    nice: i32,
    priority: u32,
    runtime: u64,
    deadline: u64,
    period: u64,
    util_min: u32,
    util_max: u32,
}

/// Asks that this thread run at no less than `percent` of a core's capacity. Returns whether
/// the kernel took it.
#[cfg(target_os = "linux")]
pub fn clamp_min(percent: u32) -> bool {
    const KEEP_POLICY: u64 = 0x08;
    const KEEP_PARAMS: u64 = 0x10;
    const UTIL_CLAMP_MIN: u64 = 0x20;
    let attr = SchedAttr {
        size: std::mem::size_of::<SchedAttr>() as u32,
        flags: KEEP_POLICY | KEEP_PARAMS | UTIL_CLAMP_MIN,
        util_min: 1024 * percent.min(100) / 100,
        util_max: 1024,
        ..SchedAttr::default()
    };
    // SAFETY: sched_setattr(0 = this thread, a fully initialised struct of the size given, 0).
    unsafe { libc::syscall(libc::SYS_sched_setattr, 0, &attr as *const SchedAttr, 0) == 0 }
}

#[cfg(not(target_os = "linux"))]
pub fn clamp_min(_percent: u32) -> bool {
    false
}
