//! Preemptive round-robin scheduler shared by every CPU.
//!
//! Each CPU keeps its own `current` slot and idle context in `arch::cpu`. The
//! task table itself is protected by one global spinlock; it is only ever
//! taken from code that runs with interrupts disabled (interrupt handlers and
//! system calls), so a CPU can never deadlock against its own timer.

pub mod elf;

use core::alloc::Layout;
use core::mem::size_of;
use core::sync::atomic::{AtomicBool, Ordering};

use alloc::string::{String, ToString};

use crate::arch::{cpu, gdt};
use crate::arch::idt::InterruptFrame;
use crate::mm::paging;

pub const KERNEL_STACK_SIZE: usize = 32 * 1024;
pub const MAX_TASKS: usize = 64;

/// A dead task's stack and address space stay untouched until this many timer
/// ticks have passed. The CPU that marked it dead is guaranteed to have
/// finished its `iretq` by then.
const REAP_GRACE_TICKS: u64 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    Unused,
    Runnable,
    Running,
    Blocked,
    Dead,
}

/// Why a task is blocked. Wakers use this to wake exactly the tasks that are
/// waiting for their event instead of broadcasting to everything.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WaitKind {
    None,
    /// Waiting for console input; the keyboard/serial IRQ wakes it.
    Console,
    /// Waiting for a child process to exit.
    Child(u64),
    /// Sleeping until an absolute monotonic nanosecond deadline.
    Until(u64),
    /// Generic event; the caller is responsible for calling `wake_pid`.
    Generic,
}

pub struct Task {
    pub pid: u64,
    pub state: State,
    pub cr3: u64,
    pub kernel_stack_top: u64,
    pub saved_rsp: u64,
    pub name: String,
    pub cwd: String,
    pub exit_code: i32,
    pub parent: u64,
    pub heap_end: u64,
    pub fpu: u64,
    /// Per-process descriptors. Children inherit a copy across `exec`.
    pub fds: crate::fs::fd::Table,
    pub uid: u32,
    pub gid: u32,
    /// 0 = SCHED_OTHER, 1 = SCHED_FIFO, 2 = SCHED_RR.
    pub policy: u8,
    /// 0 for SCHED_OTHER, 1..=99 for the real-time policies.
    pub priority: i32,
    /// Nanoseconds this task spent running.
    pub cpu_ns: u64,
    /// Monotonic timestamp when this task was last switched in.
    pub last_run_ns: u64,
    /// Absolute wake deadline for `WaitKind::Until`.
    pub wake_ns: u64,
    pub wait: WaitKind,
    /// Bytes of user address space owned by the task.
    pub mem_bytes: u64,
    preemptible: bool,
    /// True while a CPU is executing this task, so no other CPU may pick it.
    pub on_cpu: bool,
    dead_at: u64,
    reaped: bool,
}

static mut TASKS: [Option<Task>; MAX_TASKS] = [const { None }; MAX_TASKS];
static mut NEXT_PID: u64 = 1;
static mut KERNEL_CR3: u64 = 0;
static NEED_RESCHED: AtomicBool = AtomicBool::new(false);
static SCHED_LOCK: AtomicBool = AtomicBool::new(false);
static mut LAST_SWITCH_NS: [u64; crate::arch::apic::MAX_CPUS] =
    [0; crate::arch::apic::MAX_CPUS];
static mut LAST_COLLECT_NS: u64 = 0;

#[repr(align(16))]
struct Stack([u8; KERNEL_STACK_SIZE]);

static mut IDLE_STACK: Stack = Stack([0; KERNEL_STACK_SIZE]);

/// Drop guard for the global scheduler lock. Interrupts are disabled while the
/// lock is held: on a uniprocessor a timer interrupt taken inside the critical
/// section would otherwise re-enter `schedule` and spin on the lock forever.
/// The lock is recursive per CPU so a helper called from inside a critical
/// section (for example `poll_wait` from a sleep predicate) can take it again.
struct Guard {
    flags: u64,
    cpu: usize,
    outer: bool,
}

static mut LOCK_DEPTH: [u32; crate::arch::apic::MAX_CPUS] =
    [0; crate::arch::apic::MAX_CPUS];

impl Guard {
    fn acquire() -> Guard {
        let cpu = crate::arch::cpu::index();
        let flags: u64;
        unsafe {
            core::arch::asm!(
                "pushfq",
                "pop {}",
                "cli",
                out(reg) flags,
                options(nomem, nostack)
            );
        }
        let depth = unsafe { (&raw mut LOCK_DEPTH) as *mut u32 };
        let outer = unsafe {
            if core::ptr::read_volatile(depth.add(cpu)) == 0 {
                while SCHED_LOCK
                    .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                    .is_err()
                {
                    core::hint::spin_loop();
                }
                true
            } else {
                false
            }
        };
        unsafe { core::ptr::write_volatile(depth.add(cpu), core::ptr::read_volatile(depth.add(cpu)) + 1) };
        Guard { flags, cpu, outer }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let depth = unsafe { (&raw mut LOCK_DEPTH) as *mut u32 };
        unsafe {
            let d = core::ptr::read_volatile(depth.add(self.cpu));
            core::ptr::write_volatile(depth.add(self.cpu), d.saturating_sub(1));
        }
        if self.outer {
            SCHED_LOCK.store(false, Ordering::Release);
            if self.flags & (1 << 9) != 0 {
                unsafe { core::arch::asm!("sti", options(nomem, nostack)) };
            }
        }
    }
}

fn task(index: usize) -> Option<&'static mut Task> {
    if index >= MAX_TASKS {
        return None;
    }
    unsafe { (*(&raw mut TASKS))[index].as_mut() }
}

fn task_ref(index: usize) -> Option<&'static Task> {
    if index >= MAX_TASKS {
        return None;
    }
    unsafe { (*(&raw const TASKS))[index].as_ref() }
}

fn alloc_kernel_stack() -> Option<u64> {
    let layout = Layout::from_size_align(KERNEL_STACK_SIZE, 16).ok()?;
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        return None;
    }
    Some(ptr as u64 + KERNEL_STACK_SIZE as u64)
}

/// Reserve a kernel stack for an application processor's idle context.
pub fn alloc_idle_stack() -> Option<u64> {
    alloc_kernel_stack()
}

fn build_frame(
    stack_top: u64,
    cs: u64,
    ss: u64,
    rip: u64,
    rsp: u64,
    rflags: u64,
) -> u64 {
    let size = size_of::<InterruptFrame>() as u64;
    let ptr = (stack_top - size) as *mut InterruptFrame;
    unsafe {
        core::ptr::write_bytes(ptr as *mut u8, 0, size as usize);
        let f = &mut *ptr;
        f.rip = rip;
        f.cs = cs;
        f.rflags = rflags;
        f.rsp = rsp;
        f.ss = ss;
    }
    ptr as u64
}

extern "C" fn idle_entry() -> ! {
    loop {
        crate::arch::power::idle();
    }
}

pub fn init() {
    let top = (&raw mut IDLE_STACK) as *mut u8 as u64 + KERNEL_STACK_SIZE as u64;
    let frame = build_frame(
        top,
        gdt::KERNEL_CODE as u64,
        gdt::KERNEL_DATA as u64,
        idle_entry as u64,
        top,
        0x202,
    );
    let fpu_area = crate::arch::fpu::alloc_area().unwrap_or(0);
    unsafe {
        let tasks = &mut *(&raw mut TASKS);
        tasks[0] = Some(Task {
            pid: 0,
            state: State::Running,
            cr3: paging::read_cr3(),
            kernel_stack_top: top,
            saved_rsp: frame,
            name: "idle".to_string(),
            cwd: "/".to_string(),
            exit_code: 0,
            parent: 0,
            heap_end: 0,
            fpu: fpu_area,
            fds: crate::fs::fd::Table::new(),
            uid: 0,
            gid: 0,
            policy: 0,
            priority: 0,
            cpu_ns: 0,
            last_run_ns: 0,
            wake_ns: 0,
            wait: WaitKind::None,
            mem_bytes: 0,
            preemptible: true,
            on_cpu: false,
            dead_at: 0,
            reaped: false,
        });
        NEXT_PID = 1;
        KERNEL_CR3 = paging::read_cr3();
    }

    let c = cpu::get(0);
    c.current = 0;
    c.idle_stack_top = top;
    c.idle_fpu = fpu_area;
    crate::arch::fpu::set_current(fpu_area);
    NEED_RESCHED.store(false, Ordering::Relaxed);
}

pub fn start() -> ! {
    unsafe {
        let c = cpu::get(cpu::index());
        let t = task(0).unwrap();
        c.current = 0;
        c.idle_stack_top = t.kernel_stack_top;
        c.idle_fpu = t.fpu;
        c.active_cr3 = t.cr3;
        t.last_run_ns = crate::arch::timer::now_ns();
        gdt::set_kernel_stack(cpu::index(), t.kernel_stack_top);
        paging::write_cr3(t.cr3);
        crate::arch::fpu::set_current(t.fpu);
        crate::arch::context::enter_frame(t.saved_rsp as *mut InterruptFrame)
    }
}

fn current_slot() -> u64 {
    cpu::this().current
}

pub fn current_pid() -> u64 {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return 0;
    }
    task_ref(slot as usize).map(|t| t.pid).unwrap_or(0)
}

pub fn current_parent() -> u64 {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return 0;
    }
    task_ref(slot as usize).map(|t| t.parent).unwrap_or(0)
}

pub fn current_heap_end() -> u64 {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return 0;
    }
    task_ref(slot as usize).map(|t| t.heap_end).unwrap_or(0)
}

pub fn current_name() -> String {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return "idle".to_string();
    }
    task_ref(slot as usize)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| "?".to_string())
}

pub fn current_uid() -> u32 {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return 0;
    }
    task_ref(slot as usize).map(|t| t.uid).unwrap_or(0)
}

pub fn current_gid() -> u32 {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return 0;
    }
    task_ref(slot as usize).map(|t| t.gid).unwrap_or(0)
}

/// Change this task's credentials. Only root may do so.
pub fn set_current_uid(uid: u32, gid: u32) -> bool {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return false;
    }
    match task(slot as usize) {
        Some(t) if t.uid == 0 => {
            t.uid = uid;
            t.gid = gid;
            true
        }
        _ => false,
    }
}

/// Set the scheduling policy/priority of `pid`. Root or the task's owner may
/// change it; non-root callers cannot raise their own priority above 0.
pub fn set_policy(pid: u64, policy: u8, priority: i32) -> bool {
    if policy > 2 || !(0..=99).contains(&priority) {
        return false;
    }
    let caller_uid = current_uid();
    let _g = Guard::acquire();
    unsafe {
        let tasks = &mut *(&raw mut TASKS);
        let Some(t) = tasks.iter_mut().flatten().find(|t| t.pid == pid) else {
            return false;
        };
        if caller_uid != 0 && (caller_uid != t.uid || priority > 0) {
            return false;
        }
        t.policy = policy;
        t.priority = if policy == 0 { 0 } else { priority };
        true
    }
}

/// `(policy, priority)` for `pid`.
pub fn get_policy(pid: u64) -> Option<(u8, i32)> {
    let _g = Guard::acquire();
    unsafe {
        (*(&raw const TASKS))
            .iter()
            .flatten()
            .find(|t| t.pid == pid)
            .map(|t| (t.policy, t.priority))
    }
}

/// Create a user task from an ELF image. Returns the new pid.
pub fn spawn_elf(
    name: &str,
    data: &[u8],
    parent: u64,
    args: &[&str],
    cwd: String,
) -> Option<u64> {
    let cr3 = paging::new_user_cr3()?;
    let loaded = elf::load(cr3, data)?;

    if !paging::map_user_range(
        cr3,
        paging::USER_STACK_TOP - paging::USER_STACK_SIZE,
        paging::USER_STACK_SIZE,
        true,
    ) {
        return None;
    }

    let (argc, argv, user_rsp) = build_argv(cr3, args)?;

    let kernel_top = alloc_kernel_stack()?;
    let fpu_area = crate::arch::fpu::alloc_area()?;
    let frame = build_frame(
        kernel_top,
        (gdt::USER_CODE | 3) as u64,
        (gdt::USER_DATA | 3) as u64,
        loaded.entry,
        user_rsp,
        0x202,
    );
    unsafe {
        let f = &mut *(frame as *mut InterruptFrame);
        f.rdi = argc as u64;
        f.rsi = argv;
    }

    let _g = Guard::acquire();
    let (uid, gid) = unsafe {
        (*(&raw const TASKS))
            .iter()
            .flatten()
            .find(|t| t.pid == parent)
            .map(|t| (t.uid, t.gid))
            .unwrap_or((0, 0))
    };
    // The child starts with a copy of its parent's descriptors so redirection
    // set up with `dup2` survives exec.
    let fds = unsafe {
        (*(&raw const TASKS))
            .iter()
            .flatten()
            .find(|t| t.pid == parent)
            .map(|t| t.fds.clone())
            .unwrap_or_else(crate::fs::fd::Table::new)
    };
    unsafe {
        let tasks = &mut *(&raw mut TASKS);
        let slot = tasks.iter().position(|t| t.is_none())?;
        let pid = NEXT_PID;
        NEXT_PID += 1;
        tasks[slot] = Some(Task {
            pid,
            state: State::Runnable,
            cr3,
            kernel_stack_top: kernel_top,
            saved_rsp: frame,
            name: name.to_string(),
            cwd,
            exit_code: 0,
            parent,
            heap_end: loaded.end,
            fpu: fpu_area,
            fds,
            uid,
            gid,
            policy: 0,
            priority: 0,
            cpu_ns: 0,
            last_run_ns: 0,
            wake_ns: 0,
            wait: WaitKind::None,
            mem_bytes: loaded.end.saturating_sub(paging::USER_BASE),
            preemptible: false,
            on_cpu: false,
            dead_at: 0,
            reaped: false,
        });
        Some(pid)
    }
}

fn build_argv(cr3: u64, args: &[&str]) -> Option<(usize, u64, u64)> {
    let mut sp = paging::USER_STACK_TOP - 16;
    let mut pointers = alloc::vec::Vec::new();

    for arg in args.iter().rev() {
        let bytes = arg.as_bytes();
        sp -= (bytes.len() + 1) as u64;
        if !paging::write_user(cr3, sp, bytes) || !paging::write_user(cr3, sp + bytes.len() as u64, &[0]) {
            return None;
        }
        pointers.push(sp);
    }
    pointers.reverse();

    sp &= !0xF;
    let array_bytes = (pointers.len() + 1) * 8;
    sp -= array_bytes as u64;
    let argv = sp;
    for (i, ptr) in pointers.iter().enumerate() {
        if !paging::write_user(cr3, sp + i as u64 * 8, &ptr.to_le_bytes()) {
            return None;
        }
    }
    if !paging::write_user(cr3, sp + pointers.len() as u64 * 8, &0u64.to_le_bytes()) {
        return None;
    }
    let user_rsp = (sp & !0xF) - 8;
    Some((pointers.len(), argv, user_rsp))
}

pub fn spawn_path(path: &str, parent: u64) -> Option<u64> {
    let data = crate::fs::ramfs::read(path)?;
    let cwd = current_cwd();
    spawn_elf(path, data, parent, &[path], cwd)
}

pub fn spawn_with_args(path: &str, parent: u64, args: &[&str], cwd: String) -> Option<u64> {
    let data = crate::fs::ramfs::read(path)?;
    spawn_elf(path, data, parent, args, cwd)
}

pub fn current_cwd() -> String {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return "/".to_string();
    }
    task_ref(slot as usize)
        .map(|t| t.cwd.clone())
        .unwrap_or_else(|| "/".to_string())
}

pub fn set_cwd(path: String) {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return;
    }
    if let Some(t) = task(slot as usize) {
        t.cwd = path;
    }
}

/// Snapshot one descriptor of the running task.
pub fn fd_desc(fd: u64) -> Option<crate::fs::fd::Desc> {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return None;
    }
    task_ref(slot as usize)?.fds.snapshot(fd as usize)
}

/// Install a new open file at the lowest free descriptor.
pub fn fd_install_file(path: alloc::string::String, flags: u64) -> Option<u64> {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return None;
    }
    task(slot as usize)?
        .fds
        .install_file(path, flags)
        .map(|fd| fd as u64)
}

/// Install one end of a pipe at the lowest free descriptor.
pub fn fd_install_pipe(idx: usize, write: bool, cloexec: bool) -> Option<u64> {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return None;
    }
    task(slot as usize)?
        .fds
        .install_pipe(idx, write, cloexec)
        .map(|fd| fd as u64)
}

/// `dup2`, sharing one cursor or pipe end between two descriptors.
pub fn fd_dup2(old: u64, new: u64) -> bool {
    let wake = {
        let _g = Guard::acquire();
        let slot = current_slot();
        if slot == cpu::IDLE {
            return false;
        }
        match task(slot as usize).and_then(|t| t.fds.dup2(old as usize, new as usize)) {
            Some(wake) => wake,
            None => return false,
        }
    };
    for pid in wake {
        wake_pid(pid);
    }
    true
}

pub fn fd_close(fd: u64) -> bool {
    let wake = {
        let _g = Guard::acquire();
        let slot = current_slot();
        if slot == cpu::IDLE {
            return false;
        }
        match task(slot as usize).and_then(|t| t.fds.close(fd as usize)) {
            Some(wake) => wake,
            None => return false,
        }
    };
    for pid in wake {
        wake_pid(pid);
    }
    true
}

pub fn fd_set_offset(fd: u64, offset: usize) -> bool {
    let _g = Guard::acquire();
    let slot = current_slot();
    if slot == cpu::IDLE {
        return false;
    }
    task(slot as usize).is_some_and(|t| t.fds.set_offset(fd as usize, offset))
}

/// Priority-aware selection. SCHED_OTHER tasks share priority 0 and rotate
/// round-robin; SCHED_FIFO/RR tasks outrank them, and a running real-time task
/// is only displaced by a strictly higher-priority runnable task (FIFO) or by
/// an equal-priority peer taking its RR turn.
fn pick_next(cur: u64, cpu_index: usize) -> Option<usize> {
    let start = if cur == cpu::IDLE {
        1
    } else {
        (cur as usize + 1) % MAX_TASKS
    };
    let mut best: Option<usize> = None;
    let mut best_prio = i32::MIN;
    for k in 0..MAX_TASKS {
        let i = (start + k) % MAX_TASKS;
        if i == 0 {
            continue;
        }
        if let Some(t) = task(i) {
            if t.state == State::Runnable && !t.on_cpu && t.priority > best_prio {
                best_prio = t.priority;
                best = Some(i);
            }
        }
    }
    if let Some(b) = best {
        if cur != cpu::IDLE {
            if let Some(t) = task_ref(cur as usize) {
                if t.policy != 0
                    && t.state == State::Running
                    && (best_prio < t.priority
                        || (best_prio == t.priority && t.policy == 1))
                {
                    return Some(cur as usize);
                }
            }
        }
        return Some(b);
    }
    if cur == cpu::IDLE {
        return None;
    }
    let slot = cur as usize;
    if let Some(t) = task(slot) {
        if t.state == State::Running || t.state == State::Runnable {
            return Some(slot);
        }
    }
    if cpu_index == 0 {
        return Some(0);
    }
    None
}

fn switch_to(next: usize, cpu_index: usize, now: u64) -> *mut InterruptFrame {
    let t = task(next).unwrap();
    t.state = State::Running;
    t.on_cpu = true;
    t.last_run_ns = now;
    let c = cpu::get(cpu_index);
    c.current = next as u64;
    c.runs += 1;
    c.active_cr3 = t.cr3;
    gdt::set_kernel_stack(cpu_index, t.kernel_stack_top);
    paging::write_cr3(t.cr3);
    crate::arch::fpu::set_current(t.fpu);
    t.saved_rsp as *mut InterruptFrame
}

/// Restore this CPU's idle context (no runnable task left).
fn switch_to_idle(cpu_index: usize) -> *mut InterruptFrame {
    let c = cpu::get(cpu_index);
    if c.idle_rsp == 0 {
        // No idle frame to switch to: keep running the current task.
        return 0 as *mut InterruptFrame;
    }
    // Before `sched::init` has run the current CR3 *is* the kernel CR3.
    let kernel_cr3 = unsafe {
        if KERNEL_CR3 != 0 {
            KERNEL_CR3
        } else {
            paging::read_cr3()
        }
    };
    c.current = cpu::IDLE;
    c.active_cr3 = kernel_cr3;
    gdt::set_kernel_stack(cpu_index, c.idle_stack_top);
    paging::write_cr3(kernel_cr3);
    crate::arch::fpu::set_current(c.idle_fpu);
    c.idle_rsp as *mut InterruptFrame
}

fn account_current(now: u64) -> u64 {
    let slot = current_slot();
    if slot == cpu::IDLE || slot == 0 {
        return 0;
    }
    if let Some(t) = task(slot as usize) {
        let delta = if t.last_run_ns != 0 && now > t.last_run_ns {
            now - t.last_run_ns
        } else {
            0
        };
        t.cpu_ns = t.cpu_ns.saturating_add(delta);
        t.last_run_ns = now;
        return delta;
    }
    0
}

/// Save the current task and return the frame of the next one. When the same
/// task continues, its own frame is returned unchanged.
pub fn schedule(frame: *mut InterruptFrame) -> *mut InterruptFrame {
    NEED_RESCHED.store(false, Ordering::Relaxed);
    let _g = Guard::acquire();
    let cpu_index = cpu::index();
    let cur = cpu::get(cpu_index).current;
    cpu::get(cpu_index).schedules += 1;

    let now = crate::arch::timer::now_ns();
    let prev = unsafe {
        let p = (&raw mut LAST_SWITCH_NS) as *mut u64;
        let v = core::ptr::read_volatile(p.add(cpu_index));
        core::ptr::write_volatile(p.add(cpu_index), now);
        v
    };
    let delta = now.saturating_sub(prev);
    if delta > 0 {
        let busy = cur != cpu::IDLE && cur != 0;
        crate::arch::cpufreq::account(if busy { delta } else { 0 }, delta, now);
    }
    account_current(now);

    if cur != cpu::IDLE {
        if let Some(t) = task(cur as usize) {
            if t.state != State::Dead {
                t.saved_rsp = frame as u64;
            }
            if t.state == State::Running {
                t.state = State::Runnable;
            }
            // The context is saved now, so another CPU may pick it up.
            t.on_cpu = false;
        }
    } else {
        cpu::get(cpu_index).idle_rsp = frame as u64;
    }

    let result = match pick_next(cur, cpu_index) {
        Some(next) if next as u64 == cur => {
            if let Some(t) = task(next) {
                t.state = State::Running;
                t.on_cpu = true;
                t.last_run_ns = now;
            }
            frame
        }
        Some(next) => switch_to(next, cpu_index, now),
        None => {
            let idle = switch_to_idle(cpu_index);
            if idle.is_null() {
                frame
            } else {
                idle
            }
        }
    };
    crate::arch::timer::rearm();
    result
}

/// Kill the current task after a CPU fault in ring 3. Prints one line and
/// then goes through the normal exit path so the parent can wait for it.
pub fn fault_current(frame: *mut InterruptFrame, vector: usize) -> *mut InterruptFrame {
    let (pid, name) = {
        let _g = Guard::acquire();
        let slot = current_slot();
        if slot == cpu::IDLE {
            (0, alloc::string::String::from("idle"))
        } else {
            match task_ref(slot as usize) {
                Some(t) => (t.pid, t.name.clone()),
                None => (0, alloc::string::String::from("?")),
            }
        }
    };
    let faulting = unsafe { &*frame };
    if vector == 14 {
        let cr2: u64;
        unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack)) };
        crate::println!(
            "\n*** pid {} ({}) killed: {} at {:#x} (cr2={:#x}, err={:#x}) ***",
            pid,
            name,
            crate::arch::interrupts::exception_name(vector),
            faulting.rip,
            cr2,
            faulting.error
        );
    } else {
        crate::println!(
            "\n*** pid {} ({}) killed: {} at {:#x} (err={:#x}) ***",
            pid,
            name,
            crate::arch::interrupts::exception_name(vector),
            faulting.rip,
            faulting.error
        );
    }
    exit_current(frame, 128 + vector as i32)
}

pub fn exit_current(frame: *mut InterruptFrame, code: i32) -> *mut InterruptFrame {
    let info = {
        let _g = Guard::acquire();
        let slot = current_slot();
        if slot == cpu::IDLE {
            None
        } else {
            task(slot as usize).and_then(|t| {
                let pid = t.pid;
                let name = t.name.clone();
                let parent_alive = t.parent != 0 && unsafe {
                    (*(&raw const TASKS))
                        .iter()
                        .flatten()
                        .any(|p| p.pid == t.parent && p.state != State::Dead)
                };
                t.state = State::Dead;
                t.exit_code = code;
                t.dead_at = crate::arch::timer::ticks();
                t.reaped = !parent_alive;
                // Release descriptors now, not at reap time: a pipe writer
                // must close its end promptly so the reader sees EOF.
                let wake = t.fds.release_all();
                // Orphan any children and let dead ones be reclaimed; nobody
                // will ever call wait() for them now.
                unsafe {
                    for child in (*(&raw mut TASKS)).iter_mut().flatten() {
                        if child.parent == pid {
                            child.parent = 0;
                            if child.state == State::Dead {
                                child.reaped = true;
                            }
                        }
                    }
                }
                Some((pid, name, wake))
            })
        }
    };
    if let Some((pid, name, wake)) = info {
        for waiter in wake {
            wake_pid(waiter);
        }
        crate::println!("[sched] pid {} ({}) exited with {}", pid, name, code);
        wake_child(pid);
    }
    schedule(frame)
}

pub fn task_exists(pid: u64) -> bool {
    let _g = Guard::acquire();
    unsafe {
        (*(&raw const TASKS))
            .iter()
            .flatten()
            .any(|t| t.pid == pid)
    }
}

pub fn wait(pid: u64) -> Option<i32> {
    loop {
        if let Some(code) = poll_wait(pid) {
            return Some(code);
        }
        if !task_exists(pid) {
            return None;
        }
        sleep_until(WaitKind::Child(pid), || poll_wait(pid).is_some());
    }
}

pub fn poll_wait(pid: u64) -> Option<i32> {
    let _g = Guard::acquire();
    unsafe {
        let tasks = &*(&raw const TASKS);
        for t in tasks.iter().flatten() {
            if t.pid == pid {
                return if t.state == State::Dead {
                    Some(t.exit_code)
                } else {
                    None
                };
            }
        }
    }
    None
}

/// Actually release a task's address space, stack and FPU area. Only called
/// once the task is dead, no CPU has its CR3 loaded, and a grace period has
/// elapsed.
unsafe fn free_task(index: usize) -> Option<Task> {
    let slot = unsafe { &mut (*(&raw mut TASKS))[index] };
    let t = slot.take()?;
    if t.cr3 != paging::read_cr3() && !cpu::cr3_active(t.cr3) {
        paging::destroy_user_cr3(t.cr3);
    }
    if index != 0 {
        if let Ok(layout) = Layout::from_size_align(KERNEL_STACK_SIZE, 16) {
            let base = (t.kernel_stack_top - KERNEL_STACK_SIZE as u64) as *mut u8;
            unsafe { alloc::alloc::dealloc(base, layout) };
        }
        if t.fpu != 0 {
            crate::arch::fpu::forget(t.fpu);
            crate::arch::fpu::free_area(t.fpu);
        }
    }
    Some(t)
}

fn reclaimable(index: usize, now: u64) -> bool {
    match task_ref(index) {
        Some(t) if t.state == State::Dead => {
            let age = now.wrapping_sub(t.dead_at);
            if age < REAP_GRACE_TICKS {
                return false;
            }
            // An exit code must survive until the parent collects it, so a
            // dead task is only recycled once it was reaped or orphaned.
            let parent_alive = t.parent != 0
                && unsafe {
                    (*(&raw const TASKS))
                        .iter()
                        .flatten()
                        .any(|p| p.pid == t.parent && p.state != State::Dead)
                };
            (t.reaped || !parent_alive) && !t.on_cpu && !cpu::cr3_active(t.cr3)
        }
        _ => false,
    }
}

pub fn reap(pid: u64) {
    let now = crate::arch::timer::ticks();
    let _g = Guard::acquire();
    for i in 0..MAX_TASKS {
        let is_target = matches!(task_ref(i), Some(t) if t.pid == pid && t.state == State::Dead);
        if !is_target {
            continue;
        }
        if let Some(t) = task(i) {
            t.reaped = true;
        }
        if reclaimable(i, now) {
            unsafe { free_task(i) };
        }
        break;
    }
}

/// Periodic cleanup of tasks whose parent already reaped them, plus abandoned
/// zombies. Runs from the bootstrap timer interrupt.
pub fn collect_dead() {
    let now = crate::arch::timer::ticks();
    let _g = Guard::acquire();
    for i in 1..MAX_TASKS {
        if reclaimable(i, now) {
            unsafe { free_task(i) };
        }
    }
}

pub fn task_list_text() -> String {
    use core::fmt::Write;
    let mut out = String::new();
    let _g = Guard::acquire();
    unsafe {
        let tasks = &*(&raw const TASKS);
        let cur = current_slot();
        for (i, t) in tasks.iter().enumerate() {
            if let Some(t) = t {
                let state = match t.state {
                    State::Unused => "unused",
                    State::Runnable => "runnable",
                    State::Running => "running",
                    State::Blocked => "blocked",
                    State::Dead => "dead",
                };
                let _ = write!(
                    out,
                    "{:>4} {:<20} {:<9} {}\n",
                    t.pid,
                    t.name,
                    state,
                    if i as u64 == cur { "*" } else { "" }
                );
            }
        }
    }
    out.push_str(&crate::arch::cpu::stats_text());
    out
}

fn policy_name(policy: u8) -> &'static str {
    match policy {
        1 => "fifo",
        2 => "rr",
        _ => "other",
    }
}

/// Detailed per-task rows for `top`: CPU milliseconds, credentials and
/// scheduling parameters alongside the state.
pub fn task_stats_text() -> String {
    use core::fmt::Write;
    let mut out = String::new();
    let _g = Guard::acquire();
    unsafe {
        let tasks = &*(&raw const TASKS);
        let cur = current_slot();
        for (i, t) in tasks.iter().enumerate() {
            let Some(t) = t else { continue };
            let state = match t.state {
                State::Unused => "unused",
                State::Runnable => "ready",
                State::Running => "run",
                State::Blocked => "block",
                State::Dead => "dead",
            };
            let _ = write!(
                out,
                "{:>4} {:<16} {:<5} {:>4} {:>6} {:>4} {:>6} {:>8} {}\n",
                t.pid,
                t.name,
                state,
                t.uid,
                t.cpu_ns / 1_000_000,
                policy_name(t.policy),
                t.priority,
                t.mem_bytes,
                if i as u64 == cur { "*" } else { "" }
            );
        }
    }
    out
}

/// `(total tasks, runnable+running, total task cpu nanoseconds, online cpus)`.
/// The idle slot and dead tasks are excluded from the CPU total so `top` does
/// not report a permanently busy machine.
pub fn task_counts() -> (usize, usize, u64, usize) {
    let _g = Guard::acquire();
    let mut total = 0usize;
    let mut running = 0usize;
    let mut cpu_ns = 0u64;
    unsafe {
        for (i, t) in (*(&raw const TASKS)).iter().enumerate() {
            let Some(t) = t else { continue };
            if i == 0 {
                continue;
            }
            total += 1;
            if t.state == State::Running || t.state == State::Runnable {
                running += 1;
            }
            if t.state != State::Dead {
                cpu_ns = cpu_ns.saturating_add(t.cpu_ns);
            }
        }
    }
    (total, running, cpu_ns, cpu::online_count().max(1))
}

fn current_task() -> Option<&'static mut Task> {
    let slot = current_slot();
    if slot == cpu::IDLE || slot == 0 {
        None
    } else {
        task(slot as usize)
    }
}

/// Wake tasks whose sleep deadline has expired and decide whether the current
/// task has to leave the CPU. Called from the one-shot timer vector.
pub fn on_timer(now: u64) {
    let mut woke = false;
    {
        let _g = Guard::acquire();
        let mut runnable = 0usize;
        unsafe {
            for (i, slot) in (*(&raw mut TASKS)).iter_mut().enumerate() {
                let Some(t) = slot else { continue };
                if t.state == State::Runnable || t.state == State::Running {
                    if i != 0 {
                        runnable += 1;
                    }
                }
                if t.state == State::Blocked && t.wake_ns != 0 && t.wake_ns <= now {
                    t.state = State::Runnable;
                    t.wake_ns = 0;
                    t.wait = WaitKind::None;
                    woke = true;
                }
            }
        }
        let slot = current_slot();
        if slot == cpu::IDLE || slot == 0 {
            // An idle CPU must notice a task that became runnable while its
            // timer was disarmed.
            if runnable > 0 {
                NEED_RESCHED.store(true, Ordering::Relaxed);
            }
        } else if let Some(t) = task_ref(slot as usize) {
            if t.state == State::Blocked {
                NEED_RESCHED.store(true, Ordering::Relaxed);
            }
        }
    }
    if woke {
        NEED_RESCHED.store(true, Ordering::Relaxed);
    }
    let last = unsafe { LAST_COLLECT_NS };
    if now.saturating_sub(last) >= 1_000_000_000 {
        unsafe { LAST_COLLECT_NS = now };
        collect_dead();
    }
}

/// Next absolute nanosecond at which this CPU must take a timer interrupt, or
/// `None` when the timer can stay disarmed.
pub fn next_event_ns(now: u64) -> Option<u64> {
    let _g = Guard::acquire();
    let mut deadline = u64::MAX;
    let mut runnable = 0usize;
    let mut blocked_self = false;
    let cur = current_slot();
    unsafe {
        for (i, t) in (*(&raw const TASKS)).iter().enumerate() {
            let Some(t) = t else { continue };
            match t.state {
                State::Runnable => {
                    if i != 0 {
                        runnable += 1;
                    }
                }
                State::Running => {
                    if i != 0 {
                        runnable += 1;
                    }
                }
                State::Blocked => {
                    if t.wake_ns != 0 && t.wake_ns < deadline {
                        deadline = t.wake_ns;
                    }
                    if i as u64 == cur {
                        blocked_self = true;
                    }
                }
                _ => {}
            }
        }
    }
    if cur == cpu::IDLE || cur == 0 {
        if runnable > 0 {
            return Some(now + 1_000_000);
        }
    } else if blocked_self && runnable > 0 {
        return Some(now + 1_000_000);
    }
    if runnable > 1 {
        let slice = now.saturating_add(crate::arch::timer::SLICE_NS);
        if slice < deadline {
            deadline = slice;
        }
    }
    if deadline == u64::MAX { None } else { Some(deadline) }
}

pub fn rearm() {
    crate::arch::timer::rearm();
}

/// Put the current task to sleep and wake the scheduler soon.
pub fn block_current(kind: WaitKind) {
    {
        let _g = Guard::acquire();
        if let Some(t) = current_task() {
            t.state = State::Blocked;
            t.wait = kind;
            t.wake_ns = match kind {
                WaitKind::Until(d) => d,
                _ => 0,
            };
        }
        NEED_RESCHED.store(true, Ordering::Relaxed);
    }
    crate::arch::timer::kick();
}

fn wake_matching(mut pred: impl FnMut(WaitKind) -> bool) -> bool {
    let mut woke = false;
    {
        let _g = Guard::acquire();
        unsafe {
            for t in (*(&raw mut TASKS)).iter_mut().flatten() {
                if t.state == State::Blocked && pred(t.wait) {
                    t.state = State::Runnable;
                    t.wait = WaitKind::None;
                    t.wake_ns = 0;
                    woke = true;
                }
            }
        }
    }
    if woke {
        NEED_RESCHED.store(true, Ordering::Relaxed);
        broadcast_resched();
    }
    woke
}

pub fn wake_console() -> bool {
    wake_matching(|w| w == WaitKind::Console)
}

pub fn wake_child(pid: u64) -> bool {
    wake_matching(|w| w == WaitKind::Child(pid))
}

pub fn wake_pid(pid: u64) -> bool {
    let mut woke = false;
    {
        let _g = Guard::acquire();
        unsafe {
            for t in (*(&raw mut TASKS)).iter_mut().flatten() {
                if t.pid == pid && t.state == State::Blocked {
                    t.state = State::Runnable;
                    t.wait = WaitKind::None;
                    t.wake_ns = 0;
                    woke = true;
                }
            }
        }
    }
    if woke {
        NEED_RESCHED.store(true, Ordering::Relaxed);
        broadcast_resched();
    }
    woke
}

fn broadcast_resched() {
    let me = cpu::index();
    let mut i = 0;
    while i < crate::arch::apic::MAX_CPUS {
        if i != me {
            let c = cpu::get(i);
            if c.online != 0 {
                crate::arch::apic::send_ipi(c.apic_id as u32, crate::arch::apic::IPI_RESCHED);
            }
        }
        i += 1;
    }
}

pub fn yield_now() {
    NEED_RESCHED.store(true, Ordering::Relaxed);
    crate::arch::timer::kick();
}

pub fn need_resched() -> bool {
    NEED_RESCHED.load(Ordering::Relaxed)
}

/// Block until `ready` returns true. The predicate runs with the scheduler
/// lock held, so it must not block; nested scheduler helpers are safe because
/// the lock is recursive per CPU.
pub fn sleep_until(kind: WaitKind, mut ready: impl FnMut() -> bool) {
    loop {
        {
            let _g = Guard::acquire();
            if ready() {
                if let Some(t) = current_task() {
                    t.state = State::Running;
                    t.wait = WaitKind::None;
                    t.wake_ns = 0;
                }
                return;
            }
            if let Some(t) = current_task() {
                t.state = State::Blocked;
                t.wait = kind;
                t.wake_ns = match kind {
                    WaitKind::Until(d) => d,
                    _ => 0,
                };
            }
            NEED_RESCHED.store(true, Ordering::Relaxed);
        }
        crate::arch::timer::kick();
        unsafe {
            core::arch::asm!("sti", "hlt", options(nomem, nostack));
        }
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
    }
}

/// Only user contexts, idle, and explicit blocking waits are preempted.
pub fn can_preempt(frame: &InterruptFrame) -> bool {
    let slot = current_slot();
    if slot == cpu::IDLE || slot == 0 {
        return true;
    }
    if frame.cs & 3 == 3 {
        return true;
    }
    task_ref(slot as usize)
        .map(|t| t.preemptible || t.state == State::Blocked)
        .unwrap_or(true)
}
