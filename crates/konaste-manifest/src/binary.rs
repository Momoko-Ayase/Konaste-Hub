use crate::{IndexError, MAX_FILES};

pub(crate) const CONTAINER: u8 = 0x01;
pub(crate) const U32: u8 = 0x06;
pub(crate) const CLOSE: u8 = 0xfe;
const STRING: u8 = 0x0b;
const END: u8 = 0xff;
const ALPHABET: &[u8; 64] = b"0123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";
const MAX_NAME_CHARS: usize = 64;
const MAX_STRING_BYTES: usize = 2048;
const MAX_DEPTH: usize = 3;
const MAX_NODES: usize = MAX_FILES * 6 + 2;

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], IndexError> {
        let end = self.position.checked_add(count).ok_or(IndexError::Binary("section length overflow"))?;
        let value = self.bytes.get(self.position..end).ok_or(IndexError::Binary("truncated section"))?;
        self.position = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, IndexError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, IndexError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().unwrap();
        Ok(u32::from_be_bytes(bytes))
    }

    fn align(&mut self) -> Result<(), IndexError> {
        let count = (4 - self.position % 4) % 4;
        if self.take(count)?.iter().any(|byte| *byte != 0) {
            return Err(IndexError::Binary("nonzero section padding"));
        }
        Ok(())
    }

    fn finish(&self) -> Result<(), IndexError> {
        if self.position != self.bytes.len() {
            return Err(IndexError::Binary("trailing section bytes"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Names {
    /// `A0 42`: six-bit alphabet indexes, raw character count, zero low-bit pad.
    Packed,
    /// `A0 45`: raw ASCII, length byte `0x40 | (count - 1)`, no extra name padding.
    Plain,
}

pub(crate) struct Tree<'a> {
    nodes: Cursor<'a>,
    data: Cursor<'a>,
    depth: usize,
    opened: usize,
    names: Names,
}

impl<'a> Tree<'a> {
    pub(crate) fn open(bytes: &'a [u8]) -> Result<Self, IndexError> {
        let mut body = Cursor::new(bytes);
        let names = match body.take(4)? {
            [0xa0, 0x42, 0x00, 0xff] => Names::Packed,
            [0xa0, 0x45, 0x00, 0xff] => Names::Plain,
            _ => return Err(IndexError::Binary("unsupported header or encoding")),
        };
        let node_length = usize::try_from(body.u32()?).map_err(|_| IndexError::Binary("node length overflow"))?;
        if node_length == 0 || node_length % 4 != 0 {
            return Err(IndexError::Binary("unaligned node-section length"));
        }
        let nodes = Cursor::new(body.take(node_length)?);
        let data_length = usize::try_from(body.u32()?).map_err(|_| IndexError::Binary("data length overflow"))?;
        if data_length == 0 || data_length % 4 != 0 {
            return Err(IndexError::Binary("unaligned data-section length"));
        }
        let data = Cursor::new(body.take(data_length)?);
        body.finish()?;
        Ok(Self { nodes, data, depth: 0, opened: 0, names })
    }

    pub(crate) fn next_kind(&self) -> Result<u8, IndexError> {
        self.nodes.bytes.get(self.nodes.position).copied().ok_or(IndexError::Binary("missing tree terminator"))
    }

    fn name(&mut self) -> Result<String, IndexError> {
        match self.names {
            Names::Packed => self.packed_name(),
            Names::Plain => self.plain_name(),
        }
    }

    fn packed_name(&mut self) -> Result<String, IndexError> {
        let length = usize::from(self.nodes.byte()?);
        if length == 0 || length > MAX_NAME_CHARS {
            return Err(IndexError::Binary("unsupported node-name length"));
        }
        let packed = self.nodes.take((length * 6).div_ceil(8))?;
        let unused = packed.len() * 8 - length * 6;
        if unused != 0 && packed[packed.len() - 1] & ((1 << unused) - 1) != 0 {
            return Err(IndexError::Binary("nonzero node-name padding"));
        }
        let mut name = String::with_capacity(length);
        for position in 0..length {
            let bit = position * 6;
            let byte = bit / 8;
            let word = (u16::from(packed[byte]) << 8) | u16::from(packed.get(byte + 1).copied().unwrap_or(0));
            let index = ((word >> (10 - bit % 8)) & 63) as usize;
            name.push(char::from(ALPHABET[index]));
        }
        Ok(name)
    }

    fn plain_name(&mut self) -> Result<String, IndexError> {
        let length_byte = self.nodes.byte()?;
        if length_byte & 0xc0 != 0x40 {
            return Err(IndexError::Binary("unsupported node-name length"));
        }
        let length = usize::from(length_byte & 0x3f) + 1;
        if length > MAX_NAME_CHARS {
            return Err(IndexError::Binary("unsupported node-name length"));
        }
        let raw = self.nodes.take(length)?;
        if raw.iter().any(|byte| !(0x20..=0x7e).contains(byte)) {
            return Err(IndexError::Binary("node name is not printable ASCII"));
        }
        Ok(String::from_utf8(raw.to_vec()).unwrap())
    }

    fn begin(&mut self, kind: u8, name: &str) -> Result<(), IndexError> {
        if self.depth == MAX_DEPTH || self.opened == MAX_NODES {
            return Err(IndexError::Binary("tree exceeds node or depth limit"));
        }
        if self.nodes.byte()? != kind || self.name()? != name {
            return Err(IndexError::Schema("unexpected node kind, name, or order"));
        }
        self.depth += 1;
        self.opened += 1;
        Ok(())
    }

    pub(crate) fn container(&mut self, name: &str) -> Result<(), IndexError> {
        self.begin(CONTAINER, name)
    }

    pub(crate) fn u32(&mut self, name: &str) -> Result<u32, IndexError> {
        self.begin(U32, name)?;
        self.data.align()?;
        let value = self.data.u32()?;
        self.close()?;
        Ok(value)
    }

    pub(crate) fn string(&mut self, name: &str) -> Result<String, IndexError> {
        self.begin(STRING, name)?;
        self.data.align()?;
        let length = usize::try_from(self.data.u32()?).map_err(|_| IndexError::Binary("string length overflow"))?;
        if length == 0 || length > MAX_STRING_BYTES {
            return Err(IndexError::Binary("string exceeds length limit"));
        }
        let value = self.data.take(length)?;
        if value[length - 1] != 0 || value[..length - 1].iter().any(|byte| !(0x20..=0x7e).contains(byte)) {
            return Err(IndexError::Binary("string is not NUL-terminated printable ASCII"));
        }
        self.data.align()?;
        self.close()?;
        Ok(String::from_utf8(value[..length - 1].to_vec()).unwrap())
    }

    pub(crate) fn close(&mut self) -> Result<(), IndexError> {
        if self.depth == 0 || self.nodes.byte()? != CLOSE {
            return Err(IndexError::Binary("unbalanced node close"));
        }
        self.depth -= 1;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<(), IndexError> {
        if self.depth != 0 || self.nodes.byte()? != END {
            return Err(IndexError::Binary("missing or premature tree terminator"));
        }
        self.nodes.align()?;
        self.nodes.finish()?;
        self.data.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_and_depth_limits_fail_before_reading_more_nodes() {
        for (depth, opened) in [(MAX_DEPTH, 0), (0, MAX_NODES)] {
            let mut tree = Tree { nodes: Cursor::new(&[]), data: Cursor::new(&[]), depth, opened, names: Names::Packed };
            assert_eq!(tree.container("file"), Err(IndexError::Binary("tree exceeds node or depth limit")));
        }
    }
}
