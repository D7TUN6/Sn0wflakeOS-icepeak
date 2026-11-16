//! CPUID helpers shared by the timer and cpufreq code.

use core::arch::asm;

pub fn cpuid(leaf: u32, sub: u32) -> (u32, u32, u32, u32) {
    let (mut eax, mut ebx, mut ecx, mut edx);
    unsafe {
        asm!(
            "mov {tmp:e}, ebx",
            "cpuid",
            "xchg {tmp:e}, ebx",
            tmp = out(reg) ebx,
            inlateout("eax") leaf => eax,
            inlateout("ecx") sub => ecx,
            out("edx") edx,
            options(nomem, nostack)
        );
    }
    (eax, ebx, ecx, edx)
}

pub fn max_leaf() -> u32 {
    cpuid(0, 0).0
}

pub fn max_ext_leaf() -> u32 {
    cpuid(0x8000_0000, 0).0
}

pub fn vendor() -> [u8; 12] {
    let (_, b, c, d) = cpuid(0, 0);
    let mut out = [0u8; 12];
    out[0..4].copy_from_slice(&b.to_le_bytes());
    out[4..8].copy_from_slice(&d.to_le_bytes());
    out[8..12].copy_from_slice(&c.to_le_bytes());
    out
}

pub fn vendor_is(name: &[u8; 12]) -> bool {
    vendor() == *name
}

pub fn has_tsc() -> bool {
    cpuid(1, 0).3 & (1 << 4) != 0
}

pub fn has_msr() -> bool {
    cpuid(1, 0).3 & (1 << 5) != 0
}

pub fn has_apic() -> bool {
    cpuid(1, 0).3 & (1 << 9) != 0
}

pub fn has_mwait() -> bool {
    cpuid(1, 0).2 & (1 << 3) != 0
}

pub fn has_tsc_deadline() -> bool {
    cpuid(1, 0).2 & (1 << 24) != 0
}

pub fn has_invariant_tsc() -> bool {
    if max_ext_leaf() < 0x8000_0007 {
        return false;
    }
    cpuid(0x8000_0007, 0).3 & (1 << 8) != 0
}

pub fn has_eist() -> bool {
    cpuid(6, 0).2 & (1 << 0) != 0
}

pub fn has_hwp() -> bool {
    cpuid(6, 0).0 & (1 << 7) != 0
}

/// AMD hardware P-state support (CPUID.80000007H:EDX[7]).
pub fn has_hw_pstate() -> bool {
    if max_ext_leaf() < 0x8000_0007 {
        return false;
    }
    cpuid(0x8000_0007, 0).3 & (1 << 7) != 0
}

/// AMD CPPC support (CPUID.80000008H:EBX[27]).
pub fn has_cppc() -> bool {
    if max_ext_leaf() < 0x8000_0008 {
        return false;
    }
    cpuid(0x8000_0008, 0).1 & (1 << 27) != 0
}

pub fn base_freq_mhz() -> Option<u32> {
    if max_leaf() >= 0x16 {
        let (eax, _, _, _) = cpuid(0x16, 0);
        if eax != 0 {
            return Some(eax);
        }
    }
    None
}
