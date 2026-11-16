#![no_std]
#![no_main]

use icepeak_user::*;

const LINE_MAX: usize = 256;
const HIST_MAX: usize = 32;
const ESC: u8 = 0x1B;

struct History {
    entries: [[u8; LINE_MAX]; HIST_MAX],
    lens: [usize; HIST_MAX],
    count: usize,
    index: usize,
    saved: [u8; LINE_MAX],
    saved_len: usize,
}

static mut HISTORY: History = History {
    entries: [[0; LINE_MAX]; HIST_MAX],
    lens: [0; HIST_MAX],
    count: 0,
    index: 0,
    saved: [0; LINE_MAX],
    saved_len: 0,
};

fn hists() -> &'static mut History {
    unsafe { &mut *(&raw mut HISTORY) }
}

fn is_space(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

static mut LAST_STATUS: i32 = 0;

fn status() -> i32 {
    unsafe { LAST_STATUS }
}

fn set_status(code: i32) {
    unsafe { LAST_STATUS = code };
}

fn prompt() {
    let mut cwd = [0u8; 128];
    let n = getcwd(&mut cwd);
    let cwd = if n > 0 { &cwd[..n as usize] } else { b"/" };
    // Fish-style: green path, red status marker when the last command failed.
    print("\x1b[1;32m");
    let _ = write(1, cwd);
    print("\x1b[0m");
    let st = status();
    if st != 0 {
        print!("\x1b[1;31m[{}]\x1b[0m", st);
    }
    print("> ");
}

/// Longest name in `dir` (or `/bin` plus builtins) that starts with `prefix`.
/// Returns the completed token and whether it names a directory.
fn suggestion_for(line: &[u8], len: usize) -> Option<([u8; 64], usize, bool)> {
    if len == 0 {
        return None;
    }
    let cursor = len;
    let (start, _) = token_bounds(line, len, cursor);
    let first = line[..start].iter().all(|&b| is_space(b));

    let mut prefix = [0u8; 64];
    let mut dir = [0u8; 128];
    let mut dir_len = 0usize;
    let mut prefix_len = 0usize;
    let mut slash = None;
    for i in start..cursor {
        if line[i] == b'/' {
            slash = Some(i);
        }
    }
    match slash {
        Some(pos) => {
            dir_len = (pos - start + 1).min(dir.len());
            dir[..dir_len].copy_from_slice(&line[start..start + dir_len]);
            prefix_len = (cursor - pos - 1).min(prefix.len());
            prefix[..prefix_len].copy_from_slice(&line[pos + 1..pos + 1 + prefix_len]);
        }
        None => {
            prefix_len = (cursor - start).min(prefix.len());
            prefix[..prefix_len].copy_from_slice(&line[start..start + prefix_len]);
        }
    }
    if prefix_len == 0 {
        return None;
    }
    let prefix = &prefix[..prefix_len];

    let mut best = [0u8; 64];
    let mut best_len = 0usize;
    let mut best_dir = false;

    let mut consider = |name: &[u8], is_dir: bool| {
        if name.len() > prefix.len()
            && name.starts_with(prefix)
            && (best_len == 0 || name.len() < best_len)
        {
            let n = name.len().min(best.len());
            best[..n].copy_from_slice(&name[..n]);
            best_len = n;
            best_dir = is_dir;
        }
    };

    if first && dir_len == 0 {
        for builtin in ["cd", "exit", "help", "clear"] {
            consider(builtin.as_bytes(), false);
        }
    }

    let list_dir_bytes: &[u8] = if dir_len == 0 { b"/bin" } else { &dir[..dir_len] };
    let path_str = core::str::from_utf8(list_dir_bytes).unwrap_or("/bin");
    let mut buf = [0u8; 4096];
    let n = list(path_str, &mut buf);
    if n > 0 {
        let text = &buf[..n as usize];
        for row in text.split(|&b| b == b'\n') {
            if row.len() < 5 {
                continue;
            }
            let is_dir = row[0] == b'd';
            let mut i = 2;
            while i < row.len() && row[i] != b' ' {
                i += 1;
            }
            consider(&row[2..i], is_dir);
        }
    }

    if best_len == 0 {
        None
    } else {
        Some((best, best_len, best_dir))
    }
}

fn redraw(line: &[u8], len: usize, cursor: usize) {
    // Overwrite in place instead of erasing first: the console skips cells
    // whose contents did not change, so only the edited part of the line is
    // ever repainted. The erase at the end removes the tail of a longer
    // previous line.
    print("\r");
    prompt();
    let _ = write(1, &line[..len]);
    if cursor < len {
        print!("\x1b[K");
        print!("\x1b[{}D", len - cursor);
        return;
    }
    // Inline fish-style suggestion: draw the unmatched tail dim, then put the
    // hardware cursor back where the user is actually typing.
    if let Some((name, name_len, _is_dir)) = suggestion_for(line, len) {
        let (start, _) = token_bounds(line, len, cursor);
        let mut slash = None;
        for i in start..cursor {
            if line[i] == b'/' {
                slash = Some(i);
            }
        }
        let typed = match slash {
            Some(pos) => cursor - pos - 1,
            None => cursor - start,
        };
        if name_len > typed {
            let tail = &name[typed..name_len];
            print("\x1b[90m");
            let _ = write(1, tail);
            print("\x1b[0m");
            print!("\x1b[K");
            print!("\x1b[{}D", tail.len());
            return;
        }
    }
    print!("\x1b[K");
}

/// Accept the dim inline suggestion at the cursor, fish-style. Returns true if
/// anything was inserted.
fn accept_suggestion(line: &mut [u8], len: &mut usize, cursor: &mut usize) -> bool {
    if *cursor != *len {
        return false;
    }
    let Some((name, name_len, _)) = suggestion_for(line, *len) else {
        return false;
    };
    let (start, _) = token_bounds(line, *len, *cursor);
    let mut slash = None;
    for i in start..*cursor {
        if line[i] == b'/' {
            slash = Some(i);
        }
    }
    let typed = match slash {
        Some(pos) => *cursor - pos - 1,
        None => *cursor - start,
    };
    if name_len <= typed {
        return false;
    }
    let mut tail = [0u8; LINE_MAX];
    let tail_len = *len - *cursor;
    tail[..tail_len].copy_from_slice(&line[*cursor..*cursor + tail_len]);
    let extra = name_len - typed;
    if *len + extra >= LINE_MAX {
        return false;
    }
    line[*cursor..*cursor + extra].copy_from_slice(&name[typed..name_len]);
    line[*cursor + extra..*cursor + extra + tail_len].copy_from_slice(&tail[..tail_len]);
    *len += extra;
    *cursor += extra;
    redraw(line, *len, *cursor);
    true
}

fn insert(line: &mut [u8], len: &mut usize, cursor: &mut usize, b: u8) {
    if *len >= LINE_MAX - 1 {
        return;
    }
    let mut i = *len;
    while i > *cursor {
        line[i] = line[i - 1];
        i -= 1;
    }
    line[*cursor] = b;
    *len += 1;
    *cursor += 1;
}

fn delete_at(line: &mut [u8], len: &mut usize, cursor: usize) {
    if cursor >= *len {
        return;
    }
    let mut i = cursor;
    while i + 1 < *len {
        line[i] = line[i + 1];
        i += 1;
    }
    *len -= 1;
}

fn backspace(line: &mut [u8], len: &mut usize, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    delete_at(line, len, *cursor - 1);
    *cursor -= 1;
}

fn read_key() -> u8 {
    let mut b = [0u8; 1];
    loop {
        let n = read(0, &mut b);
        if n == 1 {
            return b[0];
        }
    }
}

fn add_history(line: &[u8]) {
    if line.is_empty() {
        return;
    }
    let h = hists();
    if h.count > 0 {
        let last = h.count - 1;
        if &h.entries[last][..h.lens[last]] == line {
            h.index = h.count;
            return;
        }
    }
    if h.count == HIST_MAX {
        for i in 1..HIST_MAX {
            h.entries[i - 1] = h.entries[i];
            h.lens[i - 1] = h.lens[i];
        }
        h.count -= 1;
    }
    let i = h.count;
    h.entries[i][..line.len()].copy_from_slice(line);
    h.lens[i] = line.len();
    h.count += 1;
    h.index = h.count;
}

fn load_history(delta: isize, line: &mut [u8], len: &mut usize, cursor: &mut usize) {
    let h = hists();
    if h.count == 0 {
        return;
    }
    if h.index == h.count && delta < 0 {
        h.saved[..*len].copy_from_slice(&line[..*len]);
        h.saved_len = *len;
    }
    let mut idx = h.index as isize + delta;
    if idx < 0 {
        idx = 0;
    }
    if idx > h.count as isize {
        idx = h.count as isize;
    }
    h.index = idx as usize;
    if h.index == h.count {
        *len = h.saved_len;
        line[..*len].copy_from_slice(&h.saved[..*len]);
    } else {
        *len = h.lens[h.index];
        line[..*len].copy_from_slice(&h.entries[h.index][..*len]);
    }
    *cursor = *len;
}

fn read_line() -> ([u8; LINE_MAX], usize) {
    let mut line = [0u8; LINE_MAX];
    let mut len = 0usize;
    let mut cursor = 0usize;
    redraw(&line, len, cursor);

    loop {
        let b = read_key();
        match b {
            b'\n' | b'\r' => {
                print("\n");
                return (line, len);
            }
            0x7F | 0x08 => {
                backspace(&mut line, &mut len, &mut cursor);
                redraw(&line, len, cursor);
            }
            0x09 => {
                autocomplete(&mut line, &mut len, &mut cursor);
            }
            0x01 => {
                cursor = 0;
                redraw(&line, len, cursor);
            }
            0x05 => {
                cursor = len;
                redraw(&line, len, cursor);
            }
            0x03 => {
                print("^C\n");
                len = 0;
                cursor = 0;
                return (line, 0);
            }
            0x04 => {
                if len == 0 {
                    print("\n");
                    exit(0);
                }
                delete_at(&mut line, &mut len, cursor);
                redraw(&line, len, cursor);
            }
            0x0B => {
                len = cursor;
                redraw(&line, len, cursor);
            }
            0x15 => {
                len = 0;
                cursor = 0;
                redraw(&line, len, cursor);
            }
            0x17 => {
                while cursor > 0 && is_space(line[cursor - 1]) {
                    backspace(&mut line, &mut len, &mut cursor);
                }
                while cursor > 0 && !is_space(line[cursor - 1]) {
                    backspace(&mut line, &mut len, &mut cursor);
                }
                redraw(&line, len, cursor);
            }
            0x0C => {
                clear();
                redraw(&line, len, cursor);
            }
            ESC => {
                let b2 = read_key();
                match b2 {
                    b'[' | b'O' => {
                        let b3 = read_key();
                        match b3 {
                            b'A' => {
                                load_history(-1, &mut line, &mut len, &mut cursor);
                                redraw(&line, len, cursor);
                            }
                            b'B' => {
                                load_history(1, &mut line, &mut len, &mut cursor);
                                redraw(&line, len, cursor);
                            }
                            b'C' => {
                                if cursor < len {
                                    cursor += 1;
                                    print("\x1b[C");
                                } else {
                                    accept_suggestion(&mut line, &mut len, &mut cursor);
                                }
                            }
                            b'D' => {
                                if cursor > 0 {
                                    cursor -= 1;
                                    print("\x1b[D");
                                }
                            }
                            b'H' => {
                                cursor = 0;
                                redraw(&line, len, cursor);
                            }
                            b'F' => {
                                if cursor < len {
                                    cursor = len;
                                    redraw(&line, len, cursor);
                                } else {
                                    accept_suggestion(&mut line, &mut len, &mut cursor);
                                }
                            }
                            b'3' => {
                                let _ = read_key();
                                delete_at(&mut line, &mut len, cursor);
                                redraw(&line, len, cursor);
                            }
                            _ => {}
                        }
                    }
                    b => {
                        // Alt+character: treat as a literal character.
                        if b.is_ascii_graphic() {
                            insert(&mut line, &mut len, &mut cursor, b);
                            redraw(&line, len, cursor);
                        }
                    }
                }
            }
            b if b >= 0x20 && b < 0x7F => {
                insert(&mut line, &mut len, &mut cursor, b);
                redraw(&line, len, cursor);
            }
            _ => {}
        }
    }
}

fn token_bounds(line: &[u8], len: usize, cursor: usize) -> (usize, usize) {
    let mut start = cursor;
    while start > 0 && !is_space(line[start - 1]) {
        start -= 1;
    }
    let mut end = cursor;
    while end < len && !is_space(line[end]) {
        end += 1;
    }
    (start, end)
}

fn common_prefix(matches: &[([u8; 64], usize, bool)], current: usize) -> usize {
    if matches.is_empty() {
        return 0;
    }
    let mut n = matches[0].1;
    for m in &matches[1..] {
        let mut i = 0;
        while i < n && i < m.1 && matches[0].0[i] == m.0[i] {
            i += 1;
        }
        n = i;
    }
    if n > current {
        n
    } else {
        current
    }
}

fn autocomplete(line: &mut [u8], len: &mut usize, cursor: &mut usize) {
    let (start, end) = token_bounds(line, *len, *cursor);
    let first = line[..start].iter().all(|&b| is_space(b));

    let mut prefix = [0u8; 64];
    let mut prefix_len = 0usize;
    let mut dir = [0u8; 128];
    let mut dir_len = 0usize;

    let mut slash = None;
    for i in start..*cursor {
        if line[i] == b'/' {
            slash = Some(i);
        }
    }
    match slash {
        Some(pos) => {
            dir_len = pos - start + 1;
            dir[..dir_len].copy_from_slice(&line[start..=pos]);
            prefix_len = (*cursor - pos - 1).min(prefix.len());
            prefix[..prefix_len].copy_from_slice(&line[pos + 1..pos + 1 + prefix_len]);
        }
        None => {
            prefix_len = (*cursor - start).min(prefix.len());
            prefix[..prefix_len].copy_from_slice(&line[start..start + prefix_len]);
        }
    }
    let prefix = &prefix[..prefix_len];

    let mut cwd = [0u8; 128];
    let cwd_n = getcwd(&mut cwd);
    let cwd_str = if cwd_n > 0 {
        core::str::from_utf8(&cwd[..cwd_n as usize]).unwrap_or("/")
    } else {
        "/"
    };
    let path_str = if dir_len == 0 {
        if first {
            "/bin"
        } else {
            cwd_str
        }
    } else {
        core::str::from_utf8(&dir[..dir_len]).unwrap_or("/")
    };

    let mut matches: [([u8; 64], usize, bool); 32] = [([0; 64], 0, false); 32];
    let mut count = 0usize;

    if first && dir_len == 0 {
        for builtin in ["cd", "exit", "help", "clear"] {
            if builtin.as_bytes().starts_with(prefix) && count < 32 {
                let b = builtin.as_bytes();
                matches[count].0[..b.len()].copy_from_slice(b);
                matches[count].1 = b.len();
                matches[count].2 = false;
                count += 1;
            }
        }
    }

    let mut buf = [0u8; 4096];
    let n = list(path_str, &mut buf);
    if n > 0 {
        let text = &buf[..n as usize];
        for line_b in text.split(|&b| b == b'\n') {
            if line_b.len() < 5 || count >= 32 {
                continue;
            }
            let is_dir = line_b[0] == b'd';
            let mut i = 2;
            while i < line_b.len() && line_b[i] != b' ' {
                i += 1;
            }
            let name = &line_b[2..i];
            if name.starts_with(prefix) {
                matches[count].0[..name.len()].copy_from_slice(name);
                matches[count].1 = name.len();
                matches[count].2 = is_dir;
                count += 1;
            }
        }
    }

    if count == 0 {
        return;
    }
    if count == 1 {
        let newlen = matches[0].1;
        let is_dir = matches[0].2;
        let replace_end = if *cursor < end { end } else { *cursor };
        let mut tail = [0u8; LINE_MAX];
        let tail_len = (*len - replace_end).min(LINE_MAX);
        tail[..tail_len].copy_from_slice(&line[replace_end..replace_end + tail_len]);

        for i in 0..newlen {
            line[start + i] = matches[0].0[i];
        }
        let mut pos = start + newlen;
        if is_dir {
            line[pos] = b'/';
            pos += 1;
        } else if pos >= *len || line[pos] != b' ' {
            line[pos] = b' ';
            pos += 1;
        }
        line[pos..pos + tail_len].copy_from_slice(&tail[..tail_len]);
        *len = pos + tail_len;
        *cursor = pos;
        redraw(line, *len, *cursor);
        return;
    }

    let prefix_len = common_prefix(&matches[..count], prefix.len());
    if prefix_len > prefix.len() {
        let mut tail = [0u8; LINE_MAX];
        let tail_len = (*len - *cursor).min(LINE_MAX);
        tail[..tail_len].copy_from_slice(&line[*cursor..*cursor + tail_len]);

        for i in 0..prefix_len {
            line[start + i] = matches[0].0[i];
        }
        let pos = start + prefix_len;
        line[pos..pos + tail_len].copy_from_slice(&tail[..tail_len]);
        *len = pos + tail_len;
        *cursor = pos;
        redraw(line, *len, *cursor);
        return;
    }

    print("\n");
    for m in &matches[..count] {
        let _ = write(1, &m.0[..m.1]);
        if m.2 {
            print("/");
        }
        print("  ");
    }
    print("\n");
    redraw(line, *len, *cursor);
}

fn push_byte(buf: &mut [u8; LINE_MAX], len: &mut usize, b: u8) {
    if *len < LINE_MAX - 1 {
        buf[*len] = b;
        *len += 1;
    }
}

fn push_slice(buf: &mut [u8; LINE_MAX], len: &mut usize, s: &[u8]) {
    for &b in s {
        push_byte(buf, len, b);
    }
}

/// Bytes that are special to the command tokenizer are carried as control
/// codes while quoted so they survive as literal characters.
fn quoted_byte(b: u8) -> u8 {
    match b {
        b' ' => 0x01,
        b'|' => 0x02,
        _ => b,
    }
}

fn user_name() -> &'static [u8] {
    match getuid() {
        0 => b"root",
        1000 => b"user",
        1001 => b"guest",
        65534 => b"nobody",
        _ => b"unknown",
    }
}

const MAX_VARS: usize = 16;
const VAR_NAME_MAX: usize = 24;
const VAR_VALUE_MAX: usize = 128;

struct VarTable {
    names: [[u8; VAR_NAME_MAX]; MAX_VARS],
    name_lens: [usize; MAX_VARS],
    values: [[u8; VAR_VALUE_MAX]; MAX_VARS],
    value_lens: [usize; MAX_VARS],
    count: usize,
}

static mut VARS: VarTable = VarTable {
    names: [[0; VAR_NAME_MAX]; MAX_VARS],
    name_lens: [0; MAX_VARS],
    values: [[0; VAR_VALUE_MAX]; MAX_VARS],
    value_lens: [0; MAX_VARS],
    count: 0,
};

fn set_var(name: &[u8], value: &[u8]) {
    unsafe {
        let v = &mut *(&raw mut VARS);
        let mut i = 0;
        while i < v.count {
            if v.names[i][..v.name_lens[i]] == *name {
                let n = value.len().min(VAR_VALUE_MAX);
                v.values[i][..n].copy_from_slice(&value[..n]);
                v.value_lens[i] = n;
                return;
            }
            i += 1;
        }
        if v.count < MAX_VARS && name.len() <= VAR_NAME_MAX {
            let i = v.count;
            v.names[i][..name.len()].copy_from_slice(name);
            v.name_lens[i] = name.len();
            let n = value.len().min(VAR_VALUE_MAX);
            v.values[i][..n].copy_from_slice(&value[..n]);
            v.value_lens[i] = n;
            v.count += 1;
        }
    }
}

fn get_var(name: &[u8]) -> Option<&'static [u8]> {
    unsafe {
        let v = &*(&raw const VARS);
        let mut i = 0;
        while i < v.count {
            if v.names[i][..v.name_lens[i]] == *name {
                return Some(&v.values[i][..v.value_lens[i]]);
            }
            i += 1;
        }
    }
    None
}

fn append_var(name: &[u8], out: &mut [u8; LINE_MAX], o: &mut usize) {
    if let Some(value) = get_var(name) {
        push_slice(out, o, value);
        return;
    }
    let value: &[u8] = match name {
        b"PWD" => {
            let mut cwd = [0u8; 128];
            let n = getcwd(&mut cwd);
            if n > 0 {
                // The borrow lives until the next getcwd; copy immediately.
                let n = n as usize;
                push_slice(out, o, &cwd[..n]);
                return;
            }
            b"/"
        }
        b"USER" | b"LOGNAME" => user_name(),
        b"HOME" => match getuid() {
            0 => b"/root",
            1000 => b"/home/user",
            1001 => b"/home/guest",
            _ => b"/",
        },
        b"SHELL" => b"/bin/peaksh",
        b"PATH" => b"/bin",
        b"0" => b"peaksh",
        b"UID" => {
            let uid = getuid();
            let mut digits = [0u8; 12];
            let mut d = digits.len();
            let mut v = uid as u64;
            if v == 0 {
                d -= 1;
                digits[d] = b'0';
            }
            while v > 0 {
                d -= 1;
                digits[d] = b'0' + (v % 10) as u8;
                v /= 10;
            }
            push_slice(out, o, &digits[d..]);
            return;
        }
        _ => b"",
    };
    push_slice(out, o, value);
}

/// Expand a `$...` reference at `pos`, appending its value and returning the
/// number of input bytes consumed.
fn expand_at(input: &[u8], pos: usize, out: &mut [u8; LINE_MAX], o: &mut usize) -> usize {
    if pos + 1 >= input.len() {
        push_byte(out, o, b'$');
        return 1;
    }
    let rest = &input[pos + 1..];
    if rest[0] == b'?' {
        let st = status();
        let mut digits = [0u8; 12];
        let mut d = digits.len();
        let neg = st < 0;
        let mut u = if neg {
            (st as i64).unsigned_abs() as u128
        } else {
            st as u128
        };
        if u == 0 {
            d -= 1;
            digits[d] = b'0';
        }
        while u > 0 {
            d -= 1;
            digits[d] = b'0' + (u % 10) as u8;
            u /= 10;
        }
        if neg {
            d -= 1;
            digits[d] = b'-';
        }
        push_slice(out, o, &digits[d..]);
        return 2;
    }
    if rest[0] == b'{' {
        if let Some(close) = rest.iter().position(|&b| b == b'}') {
            append_var(&rest[1..close], out, o);
            return 2 + close;
        }
        push_byte(out, o, b'$');
        return 1;
    }
    let mut n = 0usize;
    while n < rest.len() && (rest[n].is_ascii_alphanumeric() || rest[n] == b'_') {
        n += 1;
    }
    if n == 0 {
        push_byte(out, o, b'$');
        return 1;
    }
    append_var(&rest[..n], out, o);
    1 + n
}

#[derive(Clone, Copy, PartialEq)]
enum Sep {
    None,
    Semi,
    And,
    Or,
}

/// Split a raw input line into words (handling quotes, escapes and `$`
/// expansion) and run each command in a `;`, `&&` or `||` list.
fn run(input: &[u8], input_len: usize) {
    let mut cmd = [0u8; LINE_MAX];
    let mut clen = 0usize;
    let mut i = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut word_start = true;
    let mut skip = false;
    let mut last = 0i32;
    let mut sep = Sep::None;
    while i < input_len {
        let b = input[i];
        if in_single {
            if b == b'\'' {
                in_single = false;
            } else {
                push_byte(&mut cmd, &mut clen, quoted_byte(b));
            }
            i += 1;
            continue;
        }
        if in_double {
            match b {
                b'"' => {
                    in_double = false;
                    i += 1;
                }
                b'\\' if i + 1 < input_len && matches!(input[i + 1], b'"' | b'\\' | b'$' | b'`') => {
                    push_byte(&mut cmd, &mut clen, input[i + 1]);
                    i += 2;
                }
                b'$' => {
                    let before = clen;
                    i += expand_at(input, i, &mut cmd, &mut clen);
                    for b in &mut cmd[before..clen] {
                        if *b == b' ' {
                            *b = 0x01;
                        } else if *b == b'|' {
                            *b = 0x02;
                        }
                    }
                }
                _ => {
                    push_byte(&mut cmd, &mut clen, quoted_byte(b));
                    i += 1;
                }
            }
            continue;
        }
        match b {
            b'\'' => {
                in_single = true;
                word_start = false;
                i += 1;
            }
            b'"' => {
                in_double = true;
                word_start = false;
                i += 1;
            }
            b'\\' => {
                if i + 1 < input_len {
                    let e = input[i + 1];
                    push_byte(&mut cmd, &mut clen, quoted_byte(e));
                    i += 2;
                } else {
                    i += 1;
                }
                word_start = false;
            }
            b'#' if word_start => {
                while i < input_len && input[i] != b'\n' {
                    i += 1;
                }
            }
            b'~' if word_start
                && (i + 1 >= input_len || input[i + 1] == b'/' || is_space(input[i + 1])) =>
            {
                push_byte(&mut cmd, &mut clen, b'/');
                word_start = false;
                i += 1;
            }
            b';' => {
                sep = Sep::Semi;
                i += 1;
            }
            b'&' if i + 1 < input_len && input[i + 1] == b'&' => {
                sep = Sep::And;
                i += 2;
            }
            b'|' if i + 1 < input_len && input[i + 1] == b'|' => {
                sep = Sep::Or;
                i += 2;
            }
            b' ' | b'\t' | b'\n' | b'\r' => {
                if clen > 0 && cmd[clen - 1] != b' ' {
                    push_byte(&mut cmd, &mut clen, b' ');
                }
                word_start = true;
                i += 1;
            }
            b'$' => {
                i += expand_at(input, i, &mut cmd, &mut clen);
                word_start = false;
            }
            _ => {
                push_byte(&mut cmd, &mut clen, b);
                word_start = false;
                i += 1;
            }
        }
        if sep != Sep::None {
            if !skip && clen > 0 {
                last = run_command(&mut cmd, clen);
            }
            skip = match sep {
                Sep::Semi => false,
                Sep::And => last != 0,
                Sep::Or => last == 0,
                Sep::None => skip,
            };
            clen = 0;
            sep = Sep::None;
            word_start = true;
        }
    }
    if !skip && clen > 0 {
        last = run_command(&mut cmd, clen);
    }
    set_status(last);
}

/// The shell keeps private copies of its own stdin/stdout here so a command
/// can be spawned with redirected descriptors and the prompt still reaches the
/// terminal afterwards. The child inherits the redirected 0/1, not these.
const SAVED_IN: u64 = 14;
const SAVED_OUT: u64 = 15;

fn reserve_stdio() {
    let _ = dup2(STDIN, SAVED_IN);
    let _ = dup2(STDOUT, SAVED_OUT);
}

fn restore_stdio() {
    let _ = dup2(SAVED_IN, STDIN);
    let _ = dup2(SAVED_OUT, STDOUT);
}

/// Open the output side of one command or pipeline stage.
fn open_output(path: &str, truncate: bool) -> isize {
    let flags = if truncate {
        O_WRONLY | O_CREAT | O_TRUNC
    } else {
        O_WRONLY | O_CREAT | O_APPEND
    };
    open(path, flags)
}

const MAX_STAGES: usize = 8;

/// Run `A | B | C`.
///
/// Every stage is started before the first `wait`, so the stages run at the
/// same time and a producer that fills a pipe simply blocks until the consumer
/// drains it. The pipes themselves are created close-on-exec: a stage gets its
/// input and output through descriptors 0 and 1 (which `dup2` clears the mark
/// on) and does not inherit the other ends the shell is holding.
fn run_pipeline(
    argv: &[&str],
    bars: &[usize],
    out: Option<&str>,
    truncate: bool,
    input: Option<&str>,
) -> i32 {
    let stages = bars.len() + 1;
    let mut pids = [0u64; MAX_STAGES];
    let mut npids = 0usize;
    let mut stage_start = 0usize;
    let mut prev_read: Option<u64> = None;
    let mut last = 0i32;
    let mut failed = false;

    for s in 0..stages {
        let stage_end = if s < bars.len() { bars[s] } else { argv.len() };
        if stage_start >= stage_end {
            eprint("peaksh: syntax error near '|'\n");
            last = 2;
            failed = true;
            break;
        }
        let args = &argv[stage_start..stage_end];
        let is_last = s + 1 == stages;

        // Output: the redirection target for the last stage, a pipe otherwise.
        let mut next_read: Option<u64> = None;
        let mut out_fd: Option<u64> = None;
        if is_last {
            if let Some(path) = out {
                let fd = open_output(path, truncate);
                if fd < 0 {
                    eprint("peaksh: cannot write ");
                    eprint(path);
                    eprint("\n");
                    last = 1;
                    failed = true;
                    break;
                }
                out_fd = Some(fd as u64);
            }
        } else {
            match pipe2(O_CLOEXEC) {
                Some((r, w)) => {
                    next_read = Some(r);
                    out_fd = Some(w);
                }
                None => {
                    eprint("peaksh: cannot create pipe\n");
                    last = 1;
                    failed = true;
                    break;
                }
            }
        }

        // Input: the previous stage's pipe, or the `< file` of the first stage.
        let in_fd = match prev_read {
            Some(fd) => Some(fd),
            None => match input {
                Some(path) => {
                    let fd = open(path, O_RDONLY);
                    if fd < 0 {
                        eprint("peaksh: cannot read ");
                        eprint(path);
                        eprint("\n");
                        last = 1;
                        failed = true;
                        if let Some(fd) = out_fd {
                            close(fd);
                        }
                        if let Some(fd) = next_read {
                            close(fd);
                        }
                        break;
                    }
                    Some(fd as u64)
                }
                None => None,
            },
        };

        if let Some(fd) = in_fd {
            dup2(fd, STDIN);
        }
        if let Some(fd) = out_fd {
            dup2(fd, STDOUT);
        }
        let mut path_buf = [0u8; 128];
        let cmd = args[0];
        let path = if cmd.as_bytes().contains(&b'/') {
            cmd
        } else {
            make_path(&mut path_buf, "/bin/", cmd)
        };
        let pid = exec(path, args);
        restore_stdio();
        // The child has its own references now; drop the shell's copies so the
        // next stage sees end-of-file when this stage exits.
        if let Some(fd) = in_fd {
            close(fd);
        }
        if let Some(fd) = out_fd {
            close(fd);
        }
        if pid < 0 {
            eprint("peaksh: ");
            eprint(cmd);
            eprint(": command not found\n");
            last = 127;
            failed = true;
            if let Some(fd) = next_read {
                close(fd);
            }
            break;
        }
        pids[npids] = pid as u64;
        npids += 1;
        prev_read = next_read;
        stage_start = stage_end + 1;
    }

    if failed {
        // Unblock whatever was already started: closing the last read end
        // turns its writers' next write into EPIPE.
        if let Some(fd) = prev_read {
            close(fd);
        }
    }
    for i in 0..npids {
        let code = wait(pids[i]);
        if i + 1 == npids {
            last = code;
        }
    }
    set_status(last);
    last
}


fn run_command(line: &mut [u8; LINE_MAX], len: usize) -> i32 {
    if len == 0 {
        return status();
    }
    let mut starts = [0usize; 32];
    let mut ends = [0usize; 32];
    let mut count = 0usize;
    let mut i = 0usize;
    while i < len {
        while i < len && (is_space(line[i]) || line[i] == 0) {
            i += 1;
        }
        if i >= len {
            break;
        }
        let start = i;
        if line[i] == b'|' {
            i += 1;
        } else {
            while i < len && !is_space(line[i]) && line[i] != 0 && line[i] != b'|' {
                i += 1;
            }
        }
        let end = i;
        if end < LINE_MAX {
            line[end] = 0;
        }
        if count < 32 {
            starts[count] = start;
            ends[count] = end;
            count += 1;
        }
    }
    if count == 0 {
        return status();
    }

    // Quoted spaces and pipes were carried as control codes so they would not
    // split words or stages.
    for b in &mut line[..len] {
        match *b {
            0x01 => *b = b' ',
            0x02 => *b = b'|',
            _ => {}
        }
    }

    // Pull out simple redirections: `>` (truncate), `>>` (append), `<` (input).
    let mut out: Option<&str> = None;
    let mut truncate = false;
    let mut input: Option<&str> = None;
    let mut kept = 0usize;
    let mut k = 0usize;
    while k < count {
        let tok = core::str::from_utf8(&line[starts[k]..ends[k]]).unwrap_or("");
        let next = if k + 1 < count {
            Some(core::str::from_utf8(&line[starts[k + 1]..ends[k + 1]]).unwrap_or(""))
        } else {
            None
        };
        if tok == ">" || tok == ">>" {
            if let Some(path) = next {
                out = Some(path);
                truncate = tok == ">";
                k += 2;
                continue;
            }
        } else if tok == "<" {
            if let Some(path) = next {
                input = Some(path);
                k += 2;
                continue;
            }
        }
        starts[kept] = starts[k];
        ends[kept] = ends[k];
        kept += 1;
        k += 1;
    }
    count = kept;
    if count == 0 {
        return status();
    }

    let mut argv: [&str; 32] = [""; 32];
    for j in 0..count {
        argv[j] = core::str::from_utf8(&line[starts[j]..ends[j]]).unwrap_or("");
    }
    let mut argc = count;

    let mut bars = [0usize; 8];
    let mut nbars = 0usize;
    for j in 0..argc {
        if argv[j] == "|" {
            if nbars >= bars.len() {
                eprint("peaksh: too many pipes\n");
                set_status(2);
                return 2;
            }
            bars[nbars] = j;
            nbars += 1;
        }
    }
    if nbars > 0 {
        return run_pipeline(&argv[..argc], &bars[..nbars], out, truncate, input);
    }

    // POSIX-style `NAME=value` assignments. A bare list of assignments changes
    // the shell environment; a leading list before a command is applied first.
    let mut assigns = 0usize;
    while assigns < argc {
        let tok = argv[assigns].as_bytes();
        let Some(eq) = tok.iter().position(|&b| b == b'=') else {
            break;
        };
        let name = &tok[..eq];
        if name.is_empty()
            || !(name[0].is_ascii_alphabetic() || name[0] == b'_')
            || !name.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'_')
        {
            break;
        }
        set_var(name, &tok[eq + 1..]);
        assigns += 1;
    }
    if assigns > 0 {
        if assigns == argc {
            set_status(0);
            return 0;
        }
        for j in assigns..argc {
            argv[j - assigns] = argv[j];
        }
        argc -= assigns;
    }

    match argv[0] {
        "exit" => exit(0),
        "help" => {
            print("peaksh builtins: cd, clear, exit, help\n");
            print("coreutils: ls cat echo pwd mkdir rmdir rm touch cp mv ps uname sleep\n");
            print("           wc beep poweroff chmod chown stat id whoami head tail\n");
            print("           grep seq true false printf basename dirname du df free top\n");
            print("           clear chrt\n");
            print("redirection: cmd > file, cmd >> file, cmd < file\n");
            print("syntax: quotes, escapes, ; && ||, | pipes, # comments, NAME=value\n");
            print("variables: $? $PWD $USER $HOME $0 and shell assignments\n");
            set_status(0);
            return 0;
        }
        "clear" => {
            clear();
            set_status(0);
            return 0;
        }
        "cd" => {
            let target = if argc > 1 { argv[1] } else { "/" };
            let r = chdir(target);
            if r < 0 {
                eprint("peaksh: cd: ");
                print(target);
                print(": no such directory\n");
                set_status(1);
                return 1;
            }
            set_status(0);
            return 0;
        }
        _ => {}
    }

    let mut path_buf = [0u8; 128];
    let cmd = argv[0];
    let path = if cmd.as_bytes().contains(&b'/') {
        cmd
    } else {
        let p = make_path(&mut path_buf, "/bin/", cmd);
        p
    };

    let out_fd = match out {
        Some(out_path) => {
            let fd = open_output(out_path, truncate);
            if fd < 0 {
                eprint("peaksh: cannot write ");
                eprint(out_path);
                eprint("\n");
                set_status(1);
                return 1;
            }
            Some(fd)
        }
        None => None,
    };
    let in_fd = match input {
        Some(in_path) => {
            let fd = open(in_path, O_RDONLY);
            if fd < 0 {
                eprint("peaksh: cannot read ");
                eprint(in_path);
                eprint("\n");
                if let Some(fd) = out_fd {
                    close(fd as u64);
                }
                set_status(1);
                return 1;
            }
            Some(fd)
        }
        None => None,
    };
    if let Some(fd) = out_fd {
        dup2(fd as u64, STDOUT);
    }
    if let Some(fd) = in_fd {
        dup2(fd as u64, STDIN);
    }
    let pid = exec(path, &argv[..argc]);
    restore_stdio();
    if let Some(fd) = out_fd {
        close(fd as u64);
    }
    if let Some(fd) = in_fd {
        close(fd as u64);
    }
    if pid < 0 {
        eprint("peaksh: ");
        eprint(cmd);
        eprint(": command not found\n");
        set_status(127);
        return 127;
    }
    let status = wait(pid as u64);
    set_status(status);
    status
}

fn make_path<'a>(buf: &'a mut [u8; 128], prefix: &str, name: &str) -> &'a str {
    let pb = prefix.as_bytes();
    let nb = name.as_bytes();
    let n = (pb.len() + nb.len()).min(127);
    buf[..pb.len()].copy_from_slice(pb);
    buf[pb.len()..pb.len() + (n - pb.len())].copy_from_slice(&nb[..n - pb.len()]);
    core::str::from_utf8(&buf[..n]).unwrap_or("/bin/")
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(_argc: usize, _argv: *const *const u8) -> ! {
    reserve_stdio();
    print("\x1b[1;36mSn0wflakeOS peaksh\x1b[0m - type 'help' for commands\n");
    loop {
        let (line, len) = read_line();
        add_history(&line[..len]);
        run(&line, len);
    }
}
