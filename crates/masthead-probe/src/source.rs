//! A file held open and re-read from offset 0 on every sample.

use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::path::Path;

/// A reusable read buffer. It grows on first use and then keeps its size, so
/// steady-state reads do not allocate.
#[derive(Debug, Default)]
pub struct Buf {
    data: Vec<u8>,
    len: usize,
}

impl Buf {
    pub fn with_capacity(n: usize) -> Self {
        Self { data: vec![0; n.max(64)], len: 0 }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

/// An open procfs or sysfs file.
#[derive(Debug)]
pub struct Source {
    file: File,
}

impl Source {
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self { file: File::open(path)? })
    }

    /// Re-read the whole file into `buf` with positional reads from offset 0.
    pub fn read<'a>(&self, buf: &'a mut Buf) -> io::Result<&'a [u8]> {
        if buf.data.is_empty() {
            buf.data.resize(4096, 0);
        }
        let mut off = 0usize;
        loop {
            if off == buf.data.len() {
                let n = buf.data.len() * 2;
                buf.data.resize(n, 0);
            }
            let n = self.file.read_at(&mut buf.data[off..], off as u64)?;
            if n == 0 {
                break;
            }
            off += n;
        }
        buf.len = off;
        Ok(&buf.data[..off])
    }

    /// Read a file holding a single integer, such as a sysfs attribute.
    pub fn read_i64(&self, buf: &mut Buf) -> Option<i64> {
        let bytes = self.read(buf).ok()?;
        crate::parse::i64_of(crate::parse::trim(bytes))
    }

    /// Read a file holding a single word, such as `power/runtime_status`.
    pub fn read_word<'a>(&self, buf: &'a mut Buf) -> Option<&'a [u8]> {
        let bytes = self.read(buf).ok()?;
        Some(crate::parse::trim(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rereads_and_grows() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        let big = vec![b'7'; 10_000];
        std::fs::File::create(&p).unwrap().write_all(&big).unwrap();
        let s = Source::open(&p).unwrap();
        let mut b = Buf::with_capacity(16);
        assert_eq!(s.read(&mut b).unwrap().len(), 10_000);
        std::fs::write(&p, b"42\n").unwrap();
        assert_eq!(s.read_i64(&mut b), Some(42));
        assert_eq!(s.read_word(&mut b), Some(&b"42"[..]));
    }
}
