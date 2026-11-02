//! DNG (camera RAW) pictures are kept as they are; their JPEG copy is the
//! preview the camera or phone stored inside the file. Turning RAW data into
//! a picture needs a RAW developer, which the app does not carry.

use std::path::Path;

use crate::{Error, Result};

/// Write the largest JPEG preview inside the DNG `src` to `dest`, with the
/// DNG's turn flag, so it shows the right way up.
pub fn preview(src: &Path, dest: &Path) -> Result<()> {
    let b = std::fs::read(src)?;
    let (jpeg, orientation) = find(&b).ok_or_else(|| Error::Invalid("This RAW file holds no preview picture.".into()))?;
    let mut out = Vec::with_capacity(jpeg.len() + 64);
    out.extend_from_slice(&jpeg[..2]);
    if (2..=8).contains(&orientation) {
        out.extend_from_slice(&exif_orientation(orientation));
    }
    out.extend_from_slice(&jpeg[2..]);
    std::fs::write(dest, out)?;
    Ok(())
}

struct Tiff<'a> {
    b: &'a [u8],
    le: bool,
}

impl Tiff<'_> {
    fn u16(&self, at: usize) -> Option<u32> {
        let s = self.b.get(at..at + 2)?;
        Some(if self.le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) } as u32)
    }
    fn u32(&self, at: usize) -> Option<u32> {
        let s: [u8; 4] = self.b.get(at..at + 4)?.try_into().ok()?;
        Some(if self.le { u32::from_le_bytes(s) } else { u32::from_be_bytes(s) })
    }
    /// The values of an IFD entry (SHORT or LONG), as numbers.
    fn values(&self, entry: usize) -> Vec<u32> {
        let (Some(ty), Some(count)) = (self.u16(entry + 2), self.u32(entry + 4)) else { return vec![] };
        let size = match ty {
            3 => 2,
            4 | 13 => 4,
            _ => return vec![],
        };
        let count = count.min(4096) as usize;
        let base = if size * count <= 4 { entry + 8 } else { self.u32(entry + 8).unwrap_or(0) as usize };
        (0..count).filter_map(|i| if size == 2 { self.u16(base + i * 2) } else { self.u32(base + i * 4) }).collect()
    }
}

/// The largest baseline JPEG among the file's pictures, and IFD0's turn flag.
fn find(b: &[u8]) -> Option<(&[u8], u32)> {
    let le = match b.get(..4)? {
        [b'I', b'I', 42, 0] => true,
        [b'M', b'M', 0, 42] => false,
        _ => return None,
    };
    let t = Tiff { b, le };
    let mut todo = vec![t.u32(4)? as usize];
    let mut seen = std::collections::HashSet::new();
    let mut best: Option<&[u8]> = None;
    let mut orientation = 1;
    let mut first = true;
    while let Some(ifd) = todo.pop() {
        if ifd == 0 || !seen.insert(ifd) || seen.len() > 64 {
            continue;
        }
        let Some(n) = t.u16(ifd) else { continue };
        let (mut compression, mut photometric) = (0, 0);
        let (mut offsets, mut counts, mut jpeg_at, mut jpeg_len) = (vec![], vec![], 0, 0);
        for i in 0..n.min(512) as usize {
            let e = ifd + 2 + i * 12;
            let Some(tag) = t.u16(e) else { break };
            let v = t.values(e);
            let one = v.first().copied().unwrap_or(0);
            match tag {
                259 => compression = one,
                262 => photometric = one,
                273 => offsets = v,
                279 => counts = v,
                274 if first => orientation = one,
                330 => todo.extend(v.iter().map(|&o| o as usize)),
                513 => jpeg_at = one,
                514 => jpeg_len = one,
                _ => {}
            }
        }
        first = false;
        if let Some(next) = t.u32(ifd + 2 + n as usize * 12) {
            todo.push(next as usize);
        }
        let mut candidates = vec![(jpeg_at, jpeg_len)];
        // A JPEG preview in colour (YCbCr or RGB), stored as one strip; the RAW data itself is CFA or linear.
        if (compression == 6 || compression == 7) && (photometric == 6 || photometric == 2) && offsets.len() == 1 && counts.len() == 1 {
            candidates.push((offsets[0], counts[0]));
        }
        for (at, len) in candidates {
            let Some(j) = b.get(at as usize..(at as usize).saturating_add(len as usize)) else { continue };
            if len > 2 && j.starts_with(&[0xFF, 0xD8]) && is_baseline(j) && best.is_none_or(|x| j.len() > x.len()) {
                best = Some(j);
            }
        }
    }
    best.map(|j| (j, orientation))
}

/// A JPEG a browser can show (baseline or progressive, not lossless).
fn is_baseline(j: &[u8]) -> bool {
    let mut i = 2;
    while i + 4 <= j.len() {
        if j[i] != 0xFF {
            return false;
        }
        let m = j[i + 1];
        match m {
            0xC0 | 0xC1 | 0xC2 => return true,
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return false,
            0xD8 | 0x01 | 0xD0..=0xD7 => i += 2,
            _ => i += 2 + u16::from_be_bytes([j[i + 2], j[i + 3]]) as usize,
        }
    }
    false
}

/// A small EXIF block holding only the turn flag.
fn exif_orientation(o: u32) -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"MM\0\x2A\0\0\0\x08"); // big-endian, IFD0 at 8
    tiff.extend_from_slice(&1u16.to_be_bytes()); // one entry
    tiff.extend_from_slice(&274u16.to_be_bytes());
    tiff.extend_from_slice(&3u16.to_be_bytes()); // SHORT
    tiff.extend_from_slice(&1u32.to_be_bytes());
    tiff.extend_from_slice(&(o as u16).to_be_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_be_bytes()); // no next IFD
    let mut app1 = vec![0xFF, 0xE1];
    app1.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&tiff);
    app1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A little-endian TIFF: IFD0 (turn flag 6, a SubIFD) and a SubIFD holding a JPEG strip.
    fn sample(jpeg: &[u8]) -> Vec<u8> {
        let mut b = b"II\x2A\0\x08\0\0\0".to_vec();
        let entry = |b: &mut Vec<u8>, tag: u16, ty: u16, v: u32| {
            b.extend_from_slice(&tag.to_le_bytes());
            b.extend_from_slice(&ty.to_le_bytes());
            b.extend_from_slice(&1u32.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        };
        // IFD0 at 8: 2 entries, 2 + 24 + 4 = 30 bytes, so the SubIFD is at 38.
        b.extend_from_slice(&2u16.to_le_bytes());
        entry(&mut b, 274, 3, 6);
        entry(&mut b, 330, 4, 38);
        b.extend_from_slice(&0u32.to_le_bytes());
        // SubIFD at 38: 4 entries, 2 + 48 + 4 = 54 bytes, so the JPEG is at 92.
        b.extend_from_slice(&4u16.to_le_bytes());
        entry(&mut b, 259, 3, 7);
        entry(&mut b, 262, 3, 6);
        entry(&mut b, 273, 4, 92);
        entry(&mut b, 279, 4, jpeg.len() as u32);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(jpeg);
        b
    }

    #[test]
    fn finds_the_preview_and_turn_flag() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xC0, 0, 2, 0xFF, 0xD9];
        let b = sample(&jpeg);
        let (j, o) = find(&b).unwrap();
        assert_eq!((j, o), (&jpeg[..], 6));
        // Lossless JPEG (the RAW data) is not a preview.
        let raw = [0xFF, 0xD8, 0xFF, 0xC3, 0, 2, 0xFF, 0xD9];
        assert!(find(&sample(&raw)).is_none());
    }
}
