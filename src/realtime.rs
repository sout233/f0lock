/// Preserve the host's floating-point mode while enabling FTZ and DAZ on x86.
pub struct DenormalGuard {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    previous: u32,
}

impl DenormalGuard {
    pub fn new() -> Self {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            let mut previous = 0u32;
            // MXCSR is thread-local CPU state. Both buffers are valid u32s;
            // only the documented FTZ (15) and DAZ (6) control bits are changed.
            unsafe {
                std::arch::asm!("stmxcsr [{}]", in(reg) &mut previous, options(nostack, preserves_flags));
                let enabled = previous | (1 << 15) | (1 << 6);
                std::arch::asm!("ldmxcsr [{}]", in(reg) &enabled, options(nostack, preserves_flags));
            }
            Self { previous }
        }
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        Self {}
    }
}

impl Drop for DenormalGuard {
    fn drop(&mut self) {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        unsafe {
            std::arch::asm!("ldmxcsr [{}]", in(reg) &self.previous, options(nostack, preserves_flags));
        }
    }
}
