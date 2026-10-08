//! Integration with [`std::io`]. Requires the `std` feature.

use crate::BorrowedCursor;
use std::io::{self, ErrorKind, Read, Write};

/// Reads from `reader` into `cursor` with a single [`Read::read`] call.
///
/// The uninitialized part of the cursor is zeroed first, but only once over the lifetime of the
/// underlying [`BorrowedBuf`](crate::BorrowedBuf), so reading in a loop stays cheap.
///
/// # Panics
///
/// Panics if `reader` reports more bytes than the slice it was given.
///
/// ```
/// use borrowed_buf::{BorrowedBuf, io};
/// use core::mem::MaybeUninit;
///
/// let mut reader: &[u8] = b"hello world";
/// let mut storage = [MaybeUninit::uninit(); 64];
/// let mut buf = BorrowedBuf::new(&mut storage);
///
/// // Read until EOF, zeroing the buffer only once.
/// loop {
///     let mut cursor = buf.unfilled();
///     io::read_buf(&mut reader, cursor.reborrow())?;
///     if cursor.written() == 0 {
///         break;
///     }
/// }
/// assert_eq!(buf.filled(), b"hello world");
/// # Ok::<(), std::io::Error>(())
/// ```
#[inline]
pub fn read_buf<R: Read + ?Sized>(
    reader: &mut R,
    mut cursor: BorrowedCursor<'_, '_>,
) -> io::Result<()> {
    let n = reader.read(cursor.ensure_init())?;
    cursor.advance(n);
    Ok(())
}

/// Reads from `reader` until `cursor` is full.
///
/// Retries on [`ErrorKind::Interrupted`]. Returns [`ErrorKind::UnexpectedEof`] if the reader
/// ends first; whatever was read up to that point stays in the buffer.
///
/// ```
/// use borrowed_buf::{BorrowedBuf, io};
/// use core::mem::MaybeUninit;
///
/// let mut reader: &[u8] = b"\x00\x05rest";
/// let mut storage = [MaybeUninit::uninit(); 2];
/// let mut header = BorrowedBuf::new(&mut storage);
/// io::read_buf_exact(&mut reader, header.unfilled())?;
/// assert_eq!(u16::from_be_bytes([header.filled()[0], header.filled()[1]]), 5);
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn read_buf_exact<R: Read + ?Sized>(
    reader: &mut R,
    mut cursor: BorrowedCursor<'_, '_>,
) -> io::Result<()> {
    while cursor.capacity() > 0 {
        let before = cursor.written();
        match read_buf(reader, cursor.reborrow()) {
            Ok(()) if cursor.written() == before => {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "failed to fill whole buffer",
                ));
            }
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

impl Write for BorrowedCursor<'_, '_> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = buf.len().min(self.capacity());
        self.append(&buf[..n]);
        Ok(n)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
