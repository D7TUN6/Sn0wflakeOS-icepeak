use core::arch::asm;
use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::arch::apic::MAX_CPUS;
use crate::arch::port::outb;
use crate::video::{self, VideoInfo};

pub static mut SERIAL: Serial = Serial;
pub static mut TERM: Term = Term::new();

/// Serialises console output between CPUs. The lock is reentrant per CPU so a
/// fault or panic raised while printing cannot deadlock the machine.
static CONSOLE_LOCK: AtomicBool = AtomicBool::new(false);
static mut CONSOLE_NEST: [u32; MAX_CPUS] = [0; MAX_CPUS];

struct ConsoleGuard {
    idx: usize,
    owner: bool,
}

fn cpu_index_safe() -> usize {
    let value: u64;
    // Before `arch::init` the GS base is still zero, so the load reads the
    // real-mode IVT. Only trust it when it looks like a CPU number.
    unsafe { asm!("mov {}, qword ptr gs:0", out(reg) value, options(nomem, nostack)) };
    if value < MAX_CPUS as u64 {
        value as usize
    } else {
        usize::MAX
    }
}

impl ConsoleGuard {
    fn acquire() -> ConsoleGuard {
        let idx = cpu_index_safe();
        if idx != usize::MAX {
            unsafe {
                let nest = &mut (*(&raw mut CONSOLE_NEST))[idx];
                if *nest > 0 {
                    *nest += 1;
                    return ConsoleGuard { idx, owner: false };
                }
            }
        }
        while CONSOLE_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        if idx != usize::MAX {
            unsafe { (*(&raw mut CONSOLE_NEST))[idx] = 1 };
        }
        ConsoleGuard { idx, owner: true }
    }
}

impl Drop for ConsoleGuard {
    fn drop(&mut self) {
        if !self.owner {
            return;
        }
        if self.idx != usize::MAX {
            unsafe { (*(&raw mut CONSOLE_NEST))[self.idx] = 0 };
        }
        CONSOLE_LOCK.store(false, Ordering::Release);
    }
}

pub fn init() {
    unsafe {
        let serial = &mut *(&raw mut SERIAL);
        let term = &mut *(&raw mut TERM);
        serial.init();
        term.init();
    }
}

/// The framebuffer selected by the boot loader, if any.
pub fn video_info() -> Option<VideoInfo> {
    unsafe { (*(&raw const TERM)).fb }
}

pub fn _print(args: fmt::Arguments) {
    let _g = ConsoleGuard::acquire();
    unsafe {
        let serial = &mut *(&raw mut SERIAL);
        let term = &mut *(&raw mut TERM);
        let _ = serial.write_fmt(args);
        let _ = term.write_fmt(args);
        term.flush();
    }
    crate::drivers::serial::poll();
}

/// Raw byte output used by the write(2) syscall. Bytes are passed through to
/// both serial and the physical console; the console interprets a small ANSI
/// subset so shell colour and cursor control work there too.
///
/// Syscalls run on an interrupt gate, so the UART receive FIFO cannot be
/// drained by IRQ 4 while a large write is being painted. Rendering a screenful
/// takes long enough to overflow the FIFO, so the queue is drained inline
/// between chunks. Input stays ordered because the drain only enqueues events.
pub fn write_bytes(bytes: &[u8]) {
    let _g = ConsoleGuard::acquire();
    unsafe {
        let serial = &mut *(&raw mut SERIAL);
        let term = &mut *(&raw mut TERM);
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                serial.send(b'\r');
            }
            serial.send(b);
            term.put(b);
            if i % 32 == 31 {
                crate::drivers::serial::poll();
            }
        }
        term.flush();
    }
    crate::drivers::serial::poll();
}

pub fn clear() {
    let _g = ConsoleGuard::acquire();    unsafe {
        let serial = &mut *(&raw mut SERIAL);
        let term = &mut *(&raw mut TERM);
        for &b in b"\x1b[2J\x1b[H" {
            serial.send(b);
        }
        term.reset();
    }
}

pub struct Serial;

impl Serial {
    pub fn init(&mut self) {
        unsafe {
            outb(0x3F8 + 1, 0x00);
            outb(0x3F8 + 3, 0x80);
            outb(0x3F8 + 0, 0x03);
            outb(0x3F8 + 1, 0x00);
            outb(0x3F8 + 3, 0x03);
            // FIFO enabled, trigger on the first received byte so input is
            // drained promptly even when the console is busy repainting.
            outb(0x3F8 + 2, 0x07);
            outb(0x3F8 + 4, 0x0B);
        }
    }

    fn send(&mut self, b: u8) {
        unsafe {
            while crate::arch::port::inb(0x3F8 + 5) & 0x20 == 0 {}
            outb(0x3F8, b);
        }
    }
}

impl Write for Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                self.send(b'\r');
            }
            self.send(b);
        }
        Ok(())
    }
}

const VGA_BASE: usize = 0xB8000;
const VGA_COLS: usize = 80;
const VGA_ROWS: usize = 25;
const VGA_ATTR: u8 = 0x07;

const MAX_COLS: usize = 160;
const MAX_ROWS: usize = 60;
const MAX_CELLS: usize = MAX_COLS * MAX_ROWS;

/// The 16 VGA colours as 0xRRGGBB, used by the framebuffer renderer.
const PALETTE: [u32; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA, 0x555555,
    0x5555FF, 0x55FF55, 0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

/// Text console with a small ANSI escape interpreter.
///
/// It keeps a shadow cell grid and renders it either to VGA text memory or to
/// a linear framebuffer with the embedded 8x16 font. Supported: C0 controls
/// (BEL/BS/HT/LF/CR), CSI cursor movement (`A B C D E F G H f`), erase (`J K`),
/// SGR colours (`m`), cursor save/restore (`s u`, `7 8`) and cursor visibility
/// (`?25h` / `?25l`).
pub struct Term {
    row: usize,
    col: usize,
    cols: usize,
    rows: usize,
    attr: u8,
    saved_row: usize,
    saved_col: usize,
    esc: u8, // 0 = normal, 1 = after ESC, 2 = inside CSI
    params: [u16; 8],
    nparams: usize,
    param: u16,
    have_param: bool,
    private: bool,
    cursor_visible: bool,
    fb: Option<VideoInfo>,
    cursor_drawn: Option<(usize, usize)>,
    cells: [u16; MAX_CELLS],
}

impl Term {
    pub const fn new() -> Self {
        Self {
            row: 0,
            col: 0,
            cols: VGA_COLS,
            rows: VGA_ROWS,
            attr: VGA_ATTR,
            saved_row: 0,
            saved_col: 0,
            esc: 0,
            params: [0; 8],
            nparams: 0,
            param: 0,
            have_param: false,
            private: false,
            cursor_visible: true,
            fb: None,
            cursor_drawn: None,
            cells: [0; MAX_CELLS],
        }
    }

    /// Pick a backend, then clear the screen.
    pub fn init(&mut self) {
        self.fb = video::info();
        if let Some(fb) = self.fb {
            self.cols = ((fb.width / 8) as usize).min(MAX_COLS);
            self.rows = ((fb.height / 16) as usize).min(MAX_ROWS);
        } else {
            self.cols = VGA_COLS;
            self.rows = VGA_ROWS;
        }
        self.reset();
    }

    fn cells_used(&self) -> usize {
        self.cols * self.rows
    }

    fn blank_cell(&self) -> u16 {
        ((self.attr as u16) << 8) | b' ' as u16
    }

    pub fn reset(&mut self) {
        self.attr = VGA_ATTR;
        self.esc = 0;
        self.saved_row = 0;
        self.saved_col = 0;
        self.cursor_drawn = None;
        let blank = self.blank_cell();
        let used = self.cells_used();
        for cell in self.cells[..used].iter_mut() {
            *cell = blank;
        }
        if let Some(fb) = self.fb {
            let bg = fb.pack(PALETTE[((blank >> 8) as u8 >> 4) as usize]);
            video::fill_rect(
                &fb,
                0,
                0,
                fb.width,
                (self.rows * 16) as u32,
                bg,
            );
        } else {
            self.render_all();
        }
        self.row = 0;
        self.col = 0;
        self.cursor_visible = true;
        if self.fb.is_none() {
            unsafe {
                // Start address 0 so the display always shows the first row.
                outb(0x3D4, 0x0C);
                outb(0x3D5, 0x00);
                outb(0x3D4, 0x0D);
                outb(0x3D5, 0x00);
            }
        }
        self.sync_cursor();
    }

    fn draw_cell(&self, row: usize, col: usize) {
        if row >= self.rows || col >= self.cols {
            return;
        }
        let cell = self.cells[row * self.cols + col];
        if let Some(fb) = self.fb {
            self.draw_glyph(&fb, row, col, cell);
        } else {
            unsafe {
                let ptr = (VGA_BASE as *mut u16).add(row * VGA_COLS + col);
                ptr.write_volatile(cell);
            }
        }
    }

    fn draw_glyph(&self, fb: &VideoInfo, row: usize, col: usize, cell: u16) {
        let ch = (cell & 0xFF) as usize;
        let attr = (cell >> 8) as u8;
        let fg = fb.pack(PALETTE[(attr & 0x0F) as usize]);
        let bg = fb.pack(PALETTE[((attr >> 4) & 0x0F) as usize]);
        let glyph = &crate::font::FONT8X16[ch * 16..ch * 16 + 16];
        let px = (col * 8) as u32;
        let py = (row * 16) as u32;
        for (y, bits) in glyph.iter().enumerate() {
            video::blit_bits(fb, px, py + y as u32, *bits, fg, bg);
        }
    }

    fn draw_cursor(&self, row: usize, col: usize) {
        let Some(fb) = self.fb else { return };
        let cell = self.cells[row * self.cols + col];
        let attr = (cell >> 8) as u8;
        let fg = fb.pack(PALETTE[(attr & 0x0F) as usize]);
        let px = (col * 8) as u32;
        let py = (row * 16 + 14) as u32;
        for y in 0..2 {
            video::fill_row(&fb, px, py + y, 8, fg);
        }
    }

    fn render_all(&self) {
        for row in 0..self.rows {
            for col in 0..self.cols {
                self.draw_cell(row, col);
            }
        }
    }

    fn write_cell(&mut self, row: usize, col: usize, cell: u16) {
        if row >= self.rows || col >= self.cols {
            return;
        }
        let idx = row * self.cols + col;
        if self.cells[idx] == cell {
            // Repainting an unchanged cell is pure waste; the cursor overlay
            // stays where it is, which is what the caller wants anyway.
            return;
        }
        self.cells[idx] = cell;
        self.draw_cell(row, col);
    }

    fn sync_cursor(&mut self) {
        if self.fb.is_some() {
            if self.cursor_visible {
                if let Some((row, col)) = self.cursor_drawn {
                    if row != self.row || col != self.col {
                        self.draw_cell(row, col);
                    }
                }
                self.draw_cursor(self.row, self.col);
                self.cursor_drawn = Some((self.row, self.col));
            } else if let Some((row, col)) = self.cursor_drawn.take() {
                self.draw_cell(row, col);
            }
            return;
        }
        let pos = (self.row * VGA_COLS + self.col) as u16;
        unsafe {
            outb(0x3D4, 0x0F);
            outb(0x3D5, (pos >> 8) as u8);
            outb(0x3D4, 0x0E);
            outb(0x3D5, pos as u8);
            outb(0x3D4, 0x0A);
            outb(0x3D5, if self.cursor_visible { 0x0E } else { 0x20 });
            outb(0x3D4, 0x0B);
            outb(0x3D5, 0x0F);
        }
    }

    fn scroll(&mut self) {
        let cols = self.cols;
        let rows = self.rows;
        let used = cols * rows;
        let blank = self.blank_cell();
        // The cursor underline lives in the framebuffer, so it would scroll up
        // with the text and be left behind. Erase it first.
        if let Some((row, col)) = self.cursor_drawn.take() {
            self.draw_cell(row, col);
        }
        self.cells.copy_within(cols..used, 0);
        for cell in self.cells[(rows - 1) * cols..used].iter_mut() {
            *cell = blank;
        }
        if let Some(fb) = self.fb {
            // Move the visible area up one text row in the framebuffer itself
            // instead of repainting every cell from the font.
            let row_bytes = fb.pitch as usize;
            let text_px = rows * 16;
            let bytes = (text_px - 16) * row_bytes;
            unsafe {
                let base = fb.addr as usize;
                core::ptr::copy(
                    (base + 16 * row_bytes) as *const u8,
                    base as *mut u8,
                    bytes,
                );
            }
            let bg = fb.pack(PALETTE[((blank >> 8) as u8 >> 4) as usize]);
            video::fill_rect(&fb, 0, (text_px - 16) as u32, fb.width, 16, bg);
        } else {
            unsafe {
                let base = VGA_BASE as *mut u16;
                core::ptr::copy(base.add(VGA_COLS), base, (VGA_ROWS - 1) * VGA_COLS);
                for i in (VGA_ROWS - 1) * VGA_COLS..VGA_ROWS * VGA_COLS {
                    base.add(i).write_volatile(blank);
                }
            }
        }
        self.row = rows - 1;
    }

    fn newline(&mut self) {
        self.col = 0;
        self.row += 1;
        if self.row >= self.rows {
            self.scroll();
        }
    }

    fn put_raw(&mut self, b: u8) {
        if self.col >= self.cols {
            self.newline();
        }
        let cell = ((self.attr as u16) << 8) | b as u16;
        let (row, col) = (self.row, self.col);
        self.write_cell(row, col, cell);
        self.col += 1;
        if self.col >= self.cols {
            self.newline();
        }
    }

    fn put(&mut self, b: u8) {
        // Escape-sequence state machine.
        match self.esc {
            1 => {
                match b {
                    b'[' => {
                        self.esc = 2;
                        self.nparams = 0;
                        self.param = 0;
                        self.have_param = false;
                        self.private = false;
                        self.params = [0; 8];
                    }
                    b'7' => {
                        self.saved_row = self.row;
                        self.saved_col = self.col;
                        self.esc = 0;
                    }
                    b'8' => {
                        self.row = self.saved_row.min(self.rows - 1);
                        self.col = self.saved_col.min(self.cols - 1);
                        self.esc = 0;
                        self.sync_cursor();
                    }
                    b'c' => {
                        self.esc = 0;
                        self.reset();
                    }
                    _ => self.esc = 0,
                }
                return;
            }
            2 => {
                match b {
                    b'0'..=b'9' => {
                        self.param = self
                            .param
                            .saturating_mul(10)
                            .saturating_add((b - b'0') as u16);
                        self.have_param = true;
                    }
                    b';' => {
                        if self.nparams < self.params.len() {
                            self.params[self.nparams] = self.param;
                            self.nparams += 1;
                        }
                        self.param = 0;
                        self.have_param = false;
                    }
                    b'?' => self.private = true,
                    b' ' | b'>' | b'!' => {}
                    0x40..=0x7E => {
                        if self.have_param && self.nparams < self.params.len() {
                            self.params[self.nparams] = self.param;
                            self.nparams += 1;
                        }
                        self.esc = 0;
                        self.csi(b);
                    }
                    _ => self.esc = 0,
                }
                return;
            }
            _ => {}
        }

        if b == 0x1B {
            self.esc = 1;
            return;
        }

        match b {
            b'\n' => self.newline(),
            b'\r' => self.col = 0,
            b'\x08' => {
                if self.col > 0 {
                    self.col -= 1;
                } else if self.row > 0 {
                    self.row -= 1;
                    self.col = self.cols - 1;
                }
            }
            b'\t' => {
                let next = (self.col / 8 + 1) * 8;
                while self.col < next && self.col < self.cols {
                    self.put_raw(b' ');
                }
            }
            0x07 => {}
            0x20..=0x7E => self.put_raw(b),
            _ => {}
        }
    }

    /// Draw the cursor once a batch of bytes has been consumed. Doing this per
    /// byte would repaint two whole cells for every character written.
    pub fn flush(&mut self) {
        self.sync_cursor();
    }

    fn p(&self, i: usize, default: u16) -> u16 {
        match self.params.get(i) {
            Some(0) | None => default,
            Some(v) => *v,
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let blank = self.blank_cell();
        let row = self.row;
        let (start, end) = match mode {
            1 => (0, self.col + 1),
            2 => (0, self.cols),
            _ => (self.col, self.cols),
        };
        let end = end.min(self.cols);
        // Only repaint the part of the row that actually holds characters.
        // The shell erases the whole line on every keystroke, and the tail is
        // normally already blank.
        let mut first = end;
        let mut last = start;
        for x in start..end {
            if self.cells[row * self.cols + x] != blank {
                if x < first {
                    first = x;
                }
                last = x + 1;
            }
        }
        if last <= first {
            return;
        }
        let (start, end) = (first, last);
        if let Some(fb) = self.fb {
            let bg = fb.pack(PALETTE[((blank >> 8) as u8 >> 4) as usize]);
            video::fill_rect(&fb, (start * 8) as u32, (row * 16) as u32, ((end - start) * 8) as u32, 16, bg);
            for x in start..end {
                self.cells[row * self.cols + x] = blank;
            }
            return;
        }
        for x in start..end {
            self.write_cell(row, x, blank);
        }
    }

    fn erase_display(&mut self, mode: u16) {
        let blank = self.blank_cell();
        let cols = self.cols;
        let rows = self.rows;
        let used = cols * rows;
        if matches!(mode, 2 | 3) {
            for cell in self.cells[..used].iter_mut() {
                *cell = blank;
            }
            self.cursor_drawn = None;
            if let Some(fb) = self.fb {
                let bg = fb.pack(PALETTE[((blank >> 8) as u8 >> 4) as usize]);
                video::fill_rect(&fb, 0, 0, fb.width, (rows * 16) as u32, bg);
            } else {
                self.render_all();
            }
            self.row = 0;
            self.col = 0;
            return;
        }
        let (start, end) = match mode {
            1 => (0, (self.row * cols + self.col + 1).min(used)),
            _ => ((self.row * cols + self.col).min(used), used),
        };
        // Repaint only the cells that hold something, in contiguous runs.
        let bg = self
            .fb
            .map(|fb| fb.pack(PALETTE[((blank >> 8) as u8 >> 4) as usize]));
        let mut i = start;
        while i < end {
            if self.cells[i] == blank {
                i += 1;
                continue;
            }
            let run_start = i;
            while i < end && self.cells[i] != blank {
                i += 1;
            }
            for cell in self.cells[run_start..i].iter_mut() {
                *cell = blank;
            }
            match bg {
                Some(bg) => {
                    let mut k = run_start;
                    while k < i {
                        let row = k / cols;
                        let col = k % cols;
                        let n = (cols - col).min(i - k);
                        video::fill_rect(
                            self.fb.as_ref().unwrap(),
                            (col * 8) as u32,
                            (row * 16) as u32,
                            (n * 8) as u32,
                            16,
                            bg,
                        );
                        k += n;
                    }
                }
                None => {
                    for k in run_start..i {
                        self.draw_cell(k / cols, k % cols);
                    }
                }
            }
        }
    }

    fn csi(&mut self, final_byte: u8) {
        match final_byte {
            b'A' => {
                let n = self.p(0, 1) as usize;
                self.row = self.row.saturating_sub(n);
            }
            b'B' => {
                let n = self.p(0, 1) as usize;
                self.row = (self.row + n).min(self.rows - 1);
            }
            b'C' => {
                let n = self.p(0, 1) as usize;
                self.col = (self.col + n).min(self.cols - 1);
            }
            b'D' => {
                let n = self.p(0, 1) as usize;
                self.col = self.col.saturating_sub(n);
            }
            b'E' => {
                let n = self.p(0, 1) as usize;
                self.row = (self.row + n).min(self.rows - 1);
                self.col = 0;
            }
            b'F' => {
                let n = self.p(0, 1) as usize;
                self.row = self.row.saturating_sub(n);
                self.col = 0;
            }
            b'G' => {
                self.col = (self.p(0, 1) as usize - 1).min(self.cols - 1);
            }
            b'H' | b'f' => {
                self.row = (self.p(0, 1) as usize - 1).min(self.rows - 1);
                self.col = (self.p(1, 1) as usize - 1).min(self.cols - 1);
            }
            b'J' => self.erase_display(self.params.first().copied().unwrap_or(0)),
            b'K' => self.erase_line(self.params.first().copied().unwrap_or(0)),
            b'm' => self.sgr(),
            b's' => {
                self.saved_row = self.row;
                self.saved_col = self.col;
            }
            b'u' => {
                self.row = self.saved_row.min(self.rows - 1);
                self.col = self.saved_col.min(self.cols - 1);
            }
            b'h' | b'l' => {
                if self.private && self.params.first().copied() == Some(25) {
                    self.cursor_visible = final_byte == b'h';
                }
            }
            _ => {}
        }
    }

    fn sgr(&mut self) {
        if self.nparams == 0 {
            self.attr = VGA_ATTR;
            return;
        }
        let mut i = 0;
        while i < self.nparams {
            let v = self.params[i];
            match v {
                0 => self.attr = VGA_ATTR,
                1 => self.attr |= 0x08, // bold -> bright foreground
                2 => self.attr &= 0xF7, // dim
                7 => self.attr = (self.attr << 4) | (self.attr >> 4),
                22 => self.attr &= 0xF7,
                30..=37 => self.attr = (self.attr & 0xF8) | (v as u8 - 30),
                39 => self.attr = (self.attr & 0xF8) | 7,
                40..=47 => self.attr = (self.attr & 0x8F) | ((v as u8 - 40) << 4),
                49 => self.attr &= 0x8F,
                90..=97 => self.attr = (self.attr & 0xF8) | 8 | (v as u8 - 90),
                100..=107 => self.attr = (self.attr & 0x8F) | (((v as u8 - 100) + 8) << 4),
                _ => {}
            }
            i += 1;
        }
    }
}

impl Write for Term {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            self.put(b);
        }
        self.flush();
        Ok(())
    }
}
