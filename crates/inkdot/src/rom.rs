//! Loading a game: a `.gb`/`.gbc` file, or the first one in a `.zip` (DSlot shows zipped games
//! too). The zip is read from its central directory, stored or deflated (APPNOTE.TXT, the
//! format's own specification).

use std::path::Path;

/// The ROM's bytes, and its name inside a zip if it came from one.
pub fn load(path: &Path) -> Result<(Vec<u8>, Option<String>), String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if data.starts_with(b"PK\x03\x04") {
        let (name, rom) = from_zip(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok((rom, Some(name)));
    }
    Ok((data, None))
}

fn u16le(d: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32le(d: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// The first `.gb` or `.gbc` in a zip archive: its name and contents.
pub fn from_zip(zip: &[u8]) -> Result<(String, Vec<u8>), String> {
    // The end-of-central-directory record is in the last 64 KiB + 22 bytes.
    let search_from = zip.len().saturating_sub(0xffff + 22);
    let eocd = (search_from..zip.len().saturating_sub(21))
        .rev()
        .find(|&i| zip[i..].starts_with(b"PK\x05\x06"))
        .ok_or("not a zip archive")?;
    let entries = u16le(zip, eocd + 10).ok_or("damaged zip")?;
    let mut at = u32le(zip, eocd + 16).ok_or("damaged zip")?;
    for _ in 0..entries {
        if !zip.get(at..).is_some_and(|d| d.starts_with(b"PK\x01\x02")) {
            return Err("damaged zip directory".into());
        }
        let method = u16le(zip, at + 10).ok_or("damaged zip")?;
        let packed = u32le(zip, at + 20).ok_or("damaged zip")?;
        let size = u32le(zip, at + 24).ok_or("damaged zip")?;
        let name_len = u16le(zip, at + 28).ok_or("damaged zip")?;
        let extra_len = u16le(zip, at + 30).ok_or("damaged zip")?;
        let comment_len = u16le(zip, at + 32).ok_or("damaged zip")?;
        let local = u32le(zip, at + 42).ok_or("damaged zip")?;
        let name =
            String::from_utf8_lossy(zip.get(at + 46..at + 46 + name_len).ok_or("damaged zip")?)
                .into_owned();
        at += 46 + name_len + extra_len + comment_len;
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".gb") || lower.ends_with(".gbc")) {
            continue;
        }
        let lname = u16le(zip, local + 26).ok_or("damaged zip")?;
        let lextra = u16le(zip, local + 28).ok_or("damaged zip")?;
        let start = local + 30 + lname + lextra;
        let body = zip.get(start..start + packed).ok_or("damaged zip")?;
        let data = match method {
            0 => body.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(body, 16 << 20)
                .map_err(|e| format!("{name}: {e:?}"))?,
            m => return Err(format!("{name}: compression method {m} is not supported")),
        };
        if data.len() != size {
            return Err(format!("{name}: {} bytes, the zip says {size}", data.len()));
        }
        return Ok((name, data));
    }
    Err("no .gb or .gbc file in the zip".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip with one file, stored or deflated.
    fn zip(name: &str, data: &[u8], deflate: bool) -> Vec<u8> {
        let body = if deflate {
            miniz_oxide::deflate::compress_to_vec(data, 6)
        } else {
            data.to_vec()
        };
        let method: u16 = if deflate { 8 } else { 0 };
        let mut z = Vec::new();
        z.extend_from_slice(b"PK\x03\x04");
        z.extend_from_slice(&[20, 0, 0, 0]);
        z.extend_from_slice(&method.to_le_bytes());
        z.extend_from_slice(&[0; 8]); // time, date, crc (not checked)
        z.extend_from_slice(&(body.len() as u32).to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(name.as_bytes());
        z.extend_from_slice(&body);
        let dir = z.len();
        z.extend_from_slice(b"PK\x01\x02");
        z.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
        z.extend_from_slice(&method.to_le_bytes());
        z.extend_from_slice(&[0; 8]);
        z.extend_from_slice(&(body.len() as u32).to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&[0; 12]); // extra, comment, disk, attributes
        z.extend_from_slice(&0u32.to_le_bytes()); // local header at 0
        z.extend_from_slice(name.as_bytes());
        let dir_len = z.len() - dir;
        z.extend_from_slice(b"PK\x05\x06");
        z.extend_from_slice(&[0; 4]);
        z.extend_from_slice(&1u16.to_le_bytes());
        z.extend_from_slice(&1u16.to_le_bytes());
        z.extend_from_slice(&(dir_len as u32).to_le_bytes());
        z.extend_from_slice(&(dir as u32).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z
    }

    #[test]
    fn stored_and_deflated() {
        let rom: Vec<u8> = (0..40_000u32).map(|i| (i * 7 % 251) as u8).collect();
        for deflate in [false, true] {
            let (name, data) = from_zip(&zip("Game (World).gb", &rom, deflate)).unwrap();
            assert_eq!((name.as_str(), data), ("Game (World).gb", rom.clone()));
        }
    }

    #[test]
    fn a_zip_without_a_game() {
        assert!(from_zip(&zip("readme.txt", b"hello", false)).is_err());
        assert!(from_zip(b"PK\x03\x04 junk").is_err());
    }
}
