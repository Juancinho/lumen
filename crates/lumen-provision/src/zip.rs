//! Minimal zip reader for extracting named members of a Python wheel: classic (non-ZIP64)
//! archives, stored or deflated members, CRC-32 and size checked. Enough for the pinned
//! runtime wheel; anything else is rejected rather than guessed.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

#[derive(Debug)]
pub(crate) struct Entry {
    pub(crate) name: String,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    local_offset: u64,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn bad(why: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, format!("zip: {why}"))
}

/// The central directory of `file`.
pub(crate) fn entries(file: &mut File) -> std::io::Result<Vec<Entry>> {
    let len = file.seek(SeekFrom::End(0))?;
    // End of central directory: 22 bytes + up to 64 KiB of comment.
    let tail_len = len.min(22 + 65_535);
    file.seek(SeekFrom::Start(len - tail_len))?;
    let mut tail = vec![0; usize::try_from(tail_len).map_err(|_| bad("size"))?];
    file.read_exact(&mut tail)?;
    let eocd = (0..=tail.len().saturating_sub(22))
        .rev()
        .find(|&i| u32_at(&tail, i) == 0x0605_4b50)
        .ok_or_else(|| bad("no end of central directory"))?;
    let count = usize::from(u16_at(&tail, eocd + 10));
    let cd_size = u64::from(u32_at(&tail, eocd + 12));
    let cd_offset = u64::from(u32_at(&tail, eocd + 16));
    if cd_offset == 0xFFFF_FFFF || count == 0xFFFF {
        return Err(bad("ZIP64 archives are not supported"));
    }
    file.seek(SeekFrom::Start(cd_offset))?;
    let mut cd = vec![0; usize::try_from(cd_size).map_err(|_| bad("size"))?];
    file.read_exact(&mut cd)?;
    let mut out = Vec::with_capacity(count);
    let mut at = 0;
    for _ in 0..count {
        if at + 46 > cd.len() || u32_at(&cd, at) != 0x0201_4b50 {
            return Err(bad("malformed central directory"));
        }
        let name_len = usize::from(u16_at(&cd, at + 28));
        let extra_len = usize::from(u16_at(&cd, at + 30));
        let comment_len = usize::from(u16_at(&cd, at + 32));
        let name = cd
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| bad("name out of bounds"))?;
        out.push(Entry {
            name: String::from_utf8_lossy(name).into_owned(),
            method: u16_at(&cd, at + 10),
            crc: u32_at(&cd, at + 16),
            compressed: u64::from(u32_at(&cd, at + 20)),
            size: u64::from(u32_at(&cd, at + 24)),
            local_offset: u64::from(u32_at(&cd, at + 42)),
        });
        at += 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

/// Writes `entry`'s uncompressed bytes to `out`; checks size and CRC-32.
pub(crate) fn extract(file: &mut File, entry: &Entry, out: &mut dyn Write) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(entry.local_offset))?;
    let mut header = [0u8; 30];
    file.read_exact(&mut header)?;
    if u32_at(&header, 0) != 0x0403_4b50 {
        return Err(bad("malformed local header"));
    }
    let skip = u64::from(u16_at(&header, 26)) + u64::from(u16_at(&header, 28));
    file.seek(SeekFrom::Current(
        i64::try_from(skip).map_err(|_| bad("header"))?,
    ))?;
    let raw = Read::by_ref(file).take(entry.compressed);
    let mut reader: Box<dyn Read> = match entry.method {
        0 => Box::new(raw),
        8 => Box::new(flate2::read::DeflateDecoder::new(raw)),
        m => return Err(bad(&format!("compression method {m}"))),
    };
    let mut crc = crc32fast::Hasher::new();
    let mut written = 0u64;
    let mut buf = vec![0; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        crc.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        written += n as u64;
    }
    if written != entry.size || crc.finalize() != entry.crc {
        return Err(bad(&format!("{} is corrupt", entry.name)));
    }
    Ok(())
}
