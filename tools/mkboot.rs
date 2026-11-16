//! Packs stage 1, stage 2, the kernel ELF and the user modules into the boot
//! image embedded in the ISO.
//!
//! Boot image layout (all offsets in bytes):
//!   0x0000  stage 1 (2048-byte El Torito load unit)
//!   0x0300  descriptor read by stage 1
//!   0x0800  stage 2 (padded to 2048)
//!   align   archive: header, entry table, then sector-aligned payloads

use std::env;
use std::fs;
use std::process::exit;

const MAGIC: u32 = 0x534E_4631;
const SECTOR_ALIGN: usize = 2048;

fn fail(msg: &str) -> ! {
    eprintln!("mkboot: {msg}");
    exit(1);
}

fn align_up(v: usize, a: usize) -> usize {
    (v + a - 1) / a * a
}

struct Payload {
    name: String,
    kind: u32,
    data: Vec<u8>,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut stage1 = String::new();
    let mut stage2 = String::new();
    let mut kernel = String::new();
    let mut manifest = String::new();
    let mut out = String::new();
    let mut archive_out = String::new();
    let mut shift: u32 = 11;

    let mut i = 1;
    while i < args.len() {
        if i + 1 >= args.len() {
            fail("missing value");
        }
        match args[i].as_str() {
            "--stage1" => stage1 = args[i + 1].clone(),
            "--stage2" => stage2 = args[i + 1].clone(),
            "--kernel" => kernel = args[i + 1].clone(),
            "--manifest" => manifest = args[i + 1].clone(),
            "--out" => out = args[i + 1].clone(),
            "--archive-out" => archive_out = args[i + 1].clone(),
            "--shift" => shift = args[i + 1].parse().unwrap_or_else(|_| fail("bad shift")),
            other => fail(&format!("unknown argument {other}")),
        }
        i += 2;
    }
    if stage1.is_empty() || stage2.is_empty() || kernel.is_empty() || out.is_empty() {
        fail("missing required argument");
    }
    if shift != 9 && shift != 11 {
        fail("shift must be 9 or 11");
    }

    let stage1_data = fs::read(&stage1).unwrap_or_else(|_| fail("cannot read stage1"));
    let stage2_data = fs::read(&stage2).unwrap_or_else(|_| fail("cannot read stage2"));
    if stage1_data.len() > 0x800 {
        fail("stage1 exceeds 2048 bytes");
    }
    if stage2_data.len() > 0xB000 {
        fail("stage2 too large (limit 0xB000)");
    }

    let mut payloads = vec![Payload {
        name: "kernel".to_string(),
        kind: 0,
        data: fs::read(&kernel).unwrap_or_else(|_| fail("cannot read kernel")),
    }];

    if !manifest.is_empty() {
        let list = fs::read_to_string(&manifest).unwrap_or_else(|_| fail("cannot read manifest"));
        for line in list.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (name, path) = line.split_once('=').unwrap_or_else(|| fail("bad manifest line"));
            payloads.push(Payload {
                name: name.to_string(),
                kind: 1,
                data: fs::read(path).unwrap_or_else(|_| fail(&format!("cannot read {path}"))),
            });
        }
    }

    let meta_len = 12 + payloads.len() * 44;
    let data_start = align_up(meta_len, SECTOR_ALIGN);
    let mut offsets = Vec::new();
    let mut cursor = 0usize;
    for p in &payloads {
        offsets.push(data_start + cursor);
        cursor = align_up(cursor + p.data.len(), SECTOR_ALIGN);
    }
    let archive_len = data_start + cursor;

    let mut archive = vec![0u8; archive_len];
    archive[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    archive[4..8].copy_from_slice(&1u32.to_le_bytes());
    archive[8..12].copy_from_slice(&(payloads.len() as u32).to_le_bytes());
    let mut entry_off = 12;
    for (idx, p) in payloads.iter().enumerate() {
        let name = p.name.as_bytes();
        let n = name.len().min(31);
        archive[entry_off..entry_off + n].copy_from_slice(&name[..n]);
        archive[entry_off + 32..entry_off + 36].copy_from_slice(&p.kind.to_le_bytes());
        archive[entry_off + 36..entry_off + 40]
            .copy_from_slice(&(offsets[idx] as u32).to_le_bytes());
        archive[entry_off + 40..entry_off + 44]
            .copy_from_slice(&(p.data.len() as u32).to_le_bytes());
        let start = offsets[idx];
        archive[start..start + p.data.len()].copy_from_slice(&p.data);
        entry_off += 44;
    }

    let sector = 1usize << shift;
    let stage2_len = align_up(stage2_data.len(), SECTOR_ALIGN);
    let archive_off = 0x800 + stage2_len;
    let stage2_off = 0x800 / sector;
    let stage2_count = stage2_len / sector;
    let archive_off_sectors = archive_off / sector;

    let mut image = vec![0u8; archive_off + archive_len];
    image[..stage1_data.len()].copy_from_slice(&stage1_data);
    // Stage 1 reserves offsets 8..64 for the ISO boot-info table.
    for b in &mut image[8..64] {
        *b = 0;
    }
    let desc = 0x300;
    image[desc..desc + 4].copy_from_slice(&MAGIC.to_le_bytes());
    image[desc + 4..desc + 8].copy_from_slice(&shift.to_le_bytes());
    image[desc + 8..desc + 12].copy_from_slice(&(stage2_off as u32).to_le_bytes());
    image[desc + 12..desc + 16].copy_from_slice(&(stage2_count as u32).to_le_bytes());
    image[desc + 16..desc + 20].copy_from_slice(&(archive_off_sectors as u32).to_le_bytes());
    image[desc + 20..desc + 24].copy_from_slice(&(archive_len as u32).to_le_bytes());

    image[0x800..0x800 + stage2_data.len()].copy_from_slice(&stage2_data);
    image[archive_off..].copy_from_slice(&archive);

    fs::write(&out, &image).unwrap_or_else(|_| fail("cannot write output"));
    if !archive_out.is_empty() {
        fs::write(&archive_out, &archive).unwrap_or_else(|_| fail("cannot write archive"));
    }
    println!(
        "mkboot: {} payloads, {} bytes, shift {}",
        payloads.len(),
        image.len(),
        shift
    );
}
