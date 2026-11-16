//! Per-CPU state. The `GS` base points at the entry for the executing CPU so
//! hot paths can find their own task and TSS without a lookup table.

use core::arch::asm;

use super::apic::MAX_CPUS;

pub const IDLE: u64 = u64::MAX;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Cpu {
    /// Must stay first: the interrupt stubs and `this()` read `gs:0`.
    pub index: u64,
    pub apic_id: u64,
    /// Slot of the running task, or `IDLE` when this CPU has no task.
    pub current: u64,
    /// Interrupt frame of this CPU's idle loop, captured when it last left.
    pub idle_rsp: u64,
    pub idle_stack_top: u64,
    pub idle_fpu: u64,
    pub online: u64,
    pub active_cr3: u64,
    /// Number of times the global scheduler ran on this CPU.
    pub schedules: u64,
    /// Number of times this CPU switched into a real (non-idle) task.
    pub runs: u64,
}

const CPU_INIT: Cpu = Cpu {
    index: 0,
    apic_id: 0,
    current: IDLE,
    idle_rsp: 0,
    idle_stack_top: 0,
    idle_fpu: 0,
    online: 0,
    active_cr3: 0,
    schedules: 0,
    runs: 0,
};

static mut CPUS: [Cpu; MAX_CPUS] = [CPU_INIT; MAX_CPUS];

pub fn prepare(index: usize, apic_id: u32) {
    if index >= MAX_CPUS {
        return;
    }
    unsafe {
        let cpu = &mut (*(&raw mut CPUS))[index];
        cpu.index = index as u64;
        cpu.apic_id = apic_id as u64;
        cpu.current = IDLE;
        cpu.idle_rsp = 0;
        cpu.idle_stack_top = 0;
        cpu.idle_fpu = 0;
        cpu.online = 0;
        cpu.active_cr3 = 0;
        cpu.schedules = 0;
        cpu.runs = 0;
    }
}

/// Point this CPU's `GS` base at its own state block.
pub fn attach(index: usize) {
    if index >= MAX_CPUS {
        return;
    }
    let base = unsafe { (&raw mut CPUS).cast::<Cpu>().add(index) } as u64;
    set_gs_base(base);
}

pub fn init(index: usize, apic_id: u32) {
    prepare(index, apic_id);
    attach(index);
}

pub fn set_gs_base(base: u64) {
    // IA32_GS_BASE
    super::apic::write_msr(0xC000_0101, base);
}

/// Logical CPU number from `gs:0`, clamped to a valid index. Before `attach`
/// (and immediately after an AP reloads its GS selector) the slot holds
/// whatever the real-mode IVT contains, so an unchecked value would index far
/// outside `CPUS`.
#[inline]
fn gs_index() -> usize {
    let p: u64;
    unsafe { asm!("mov {}, qword ptr gs:0", out(reg) p, options(nomem, nostack)) };
    if p < MAX_CPUS as u64 { p as usize } else { 0 }
}

pub fn this() -> &'static mut Cpu {
    unsafe { &mut *((&raw mut CPUS).cast::<Cpu>().add(gs_index())) }
}

pub fn index() -> usize {
    gs_index()
}

pub fn get(index: usize) -> &'static mut Cpu {
    unsafe { &mut *((&raw mut CPUS).cast::<Cpu>().add(index.min(MAX_CPUS - 1))) }
}

pub fn mark_online(index: usize, apic_id: u32) {
    let cpu = get(index);
    cpu.apic_id = apic_id as u64;
    cpu.online = 1;
}

/// Volatile check so a polling CPU actually re-reads what an AP published.
pub fn is_online(index: usize) -> bool {
    if index >= MAX_CPUS {
        return false;
    }
    unsafe { core::ptr::read_volatile(&get(index).online) != 0 }
}

pub fn online_count() -> usize {
    unsafe {
        let cpus = &*(&raw const CPUS);
        cpus.iter().filter(|c| c.online != 0).count()
    }
}

/// True when any CPU currently has `cr3` loaded.
pub fn cr3_active(cr3: u64) -> bool {
    unsafe {
        let cpus = &*(&raw const CPUS);
        cpus.iter().any(|c| c.active_cr3 == cr3)
    }
}

/// Per-CPU activity table, appended to the `ps` output.
pub fn stats_text() -> alloc::string::String {
    use core::fmt::Write;
    let mut out = alloc::string::String::new();
    out.push_str("\n  CPU APIC ONLINE SCHED RUNS\n");
    unsafe {
        let cpus = &*(&raw const CPUS);
        for (i, c) in cpus.iter().enumerate() {
            if c.online == 0 {
                continue;
            }
            let _ = write!(
                out,
                "  {:>3} {:>4} {:>6} {:>5} {:>4}\n",
                i, c.apic_id, c.online, c.schedules, c.runs
            );
        }
    }
    out
}
