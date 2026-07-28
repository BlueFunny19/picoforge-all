//! Minimal BER-TLV reader/writer for applet data objects.
//!
//! Handles multi-byte tags (e.g. `0x7F49`, `0x5FC105`) and multi-byte lengths,
//! which the rescue module's hand-rolled 1-byte-length loops cannot parse. Tags
//! are carried as `u32` (their big-endian byte sequence), which covers every tag
//! the OATH/PIV/OpenPGP applets use.

/// Iterator over the `(tag, value)` pairs in a BER-TLV byte string.
pub struct TlvIter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> TlvIter<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl<'a> Iterator for TlvIter<'a> {
    type Item = (u32, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let tag = read_tag(self.data, &mut self.pos)?;
        let len = read_len(self.data, &mut self.pos)?;
        let end = self.pos.checked_add(len)?;
        if end > self.data.len() {
            return None;
        }
        let value = &self.data[self.pos..end];
        self.pos = end;
        Some((tag, value))
    }
}

/// Read a BER tag starting at `*pos`, advancing it past the tag bytes.
pub fn read_tag(data: &[u8], pos: &mut usize) -> Option<u32> {
    let first = *data.get(*pos)?;
    *pos += 1;
    let mut tag = first as u32;
    // Multi-byte tag: low 5 bits all set, continuation while high bit set.
    if first & 0x1F == 0x1F {
        loop {
            let b = *data.get(*pos)?;
            *pos += 1;
            tag = (tag << 8) | b as u32;
            if b & 0x80 == 0 {
                break;
            }
        }
    }
    Some(tag)
}

/// Read a BER length starting at `*pos`, advancing it past the length bytes.
pub fn read_len(data: &[u8], pos: &mut usize) -> Option<usize> {
    let first = *data.get(*pos)?;
    *pos += 1;
    if first & 0x80 == 0 {
        return Some(first as usize);
    }
    let nbytes = (first & 0x7F) as usize;
    if nbytes == 0 || nbytes > 4 {
        return None; // indefinite form / oversized — not used by these applets
    }
    let mut len = 0usize;
    for _ in 0..nbytes {
        let b = *data.get(*pos)?;
        *pos += 1;
        len = (len << 8) | b as usize;
    }
    Some(len)
}

/// First value carrying `tag` at the top level of `data`, if present.
pub fn find(data: &[u8], tag: u32) -> Option<&[u8]> {
    TlvIter::new(data).find(|(t, _)| *t == tag).map(|(_, v)| v)
}

/// Append a TLV (`tag` as its minimal big-endian bytes, BER length, value).
pub fn write(out: &mut Vec<u8>, tag: u32, value: &[u8]) {
    // Tag: strip leading zero bytes, but keep at least one.
    let tag_bytes = tag.to_be_bytes();
    let start = tag_bytes.iter().position(|&b| b != 0).unwrap_or(3);
    out.extend_from_slice(&tag_bytes[start..]);
    write_len(out, value.len());
    out.extend_from_slice(value);
}

/// Append a BER length octet sequence.
pub fn write_len(out: &mut Vec<u8>, len: usize) {
    if len < 0x80 {
        out.push(len as u8);
    } else if len <= 0xFF {
        out.push(0x81);
        out.push(len as u8);
    } else if len <= 0xFFFF {
        out.push(0x82);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0x83);
        out.extend_from_slice(&(len as u32).to_be_bytes()[1..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_single_byte_tags() {
        let mut buf = Vec::new();
        write(&mut buf, 0x71, b"Example:alice");
        write(&mut buf, 0x73, &[0x21, 0x06, 0xDE, 0xAD]);
        let items: Vec<_> = TlvIter::new(&buf).collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], (0x71, b"Example:alice".as_slice()));
        assert_eq!(items[1], (0x73, [0x21, 0x06, 0xDE, 0xAD].as_slice()));
    }

    #[test]
    fn parses_multi_byte_tag_and_len() {
        // Tag 0x7F49, length 0x81 0x80 (128 bytes).
        let mut buf = vec![0x7F, 0x49, 0x81, 0x80];
        buf.extend(std::iter::repeat_n(0xAB, 128));
        let (tag, val) = TlvIter::new(&buf).next().unwrap();
        assert_eq!(tag, 0x7F49);
        assert_eq!(val.len(), 128);
    }

    #[test]
    fn write_encodes_extended_length() {
        let mut buf = Vec::new();
        write(&mut buf, 0x5FC105, &vec![0u8; 300]);
        assert_eq!(&buf[..6], &[0x5F, 0xC1, 0x05, 0x82, 0x01, 0x2C]);
    }

    #[test]
    fn find_returns_first_match() {
        let mut buf = Vec::new();
        write(&mut buf, 0x71, b"one");
        write(&mut buf, 0x71, b"two");
        assert_eq!(find(&buf, 0x71), Some(b"one".as_slice()));
        assert_eq!(find(&buf, 0x99), None);
    }

    #[test]
    fn truncated_value_yields_no_item() {
        // Claims length 5 but only 2 bytes follow.
        let buf = [0x71, 0x05, 0xAA, 0xBB];
        assert_eq!(TlvIter::new(&buf).next(), None);
    }
}
