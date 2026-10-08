# borrowed-buf

A small, fast, `no_std` alternative to the nightly-only
[`core::io::BorrowedBuf`](https://doc.rust-lang.org/nightly/core/io/struct.BorrowedBuf.html)
and `BorrowedCursor`, usable on **stable Rust (MSRV 1.89)**.

It lets you read into uninitialized memory without zeroing it first, while
tracking how much of the buffer is filled and how much is initialized:

```text
[             capacity              ]
[ filled |         unfilled         ]
[    initialized    | uninitialized ]
```

- No dependencies, `#![no_std]`; the default `std` feature adds `std::io` helpers.
- Generic over the element type: `BorrowedBuf<'a, T = u8>` for any `T: Copy`.
- The initialized region only grows, so a reused buffer is zeroed at most once.
- Safe accessors for the initialized and uninitialized halves of the unfilled region.
- Every test runs under Miri (Stacked Borrows and Tree Borrows) in CI.

## Why

Most APIs that fill a buffer (`Read::read`, decoders, codecs) take `&mut [T]`,
which must already be initialized. That leaves two options:

- **Zero it first.** `[0; N]` or `vec![0; n]` costs a full memset for every
  fresh buffer, often more than the read itself.
- **Use `MaybeUninit<T>` by hand.** Then you track which prefix is written,
  call `assume_init` only on that prefix, and never create a `&[T]` over
  uninitialized memory. Get any step wrong and it's silent undefined behavior.

`BorrowedBuf` is a **safe wrapper around `&mut [MaybeUninit<T>]`** that does
this bookkeeping for you:

- `filled()` and `into_filled()` expose only initialized data, as plain
  `&[T]` / `&mut [T]`, so your code never calls `assume_init`.
- `append()` copies straight into uninitialized memory, with no zeroing.
- `ensure_init()` initializes only what was never initialized, at most once per
  buffer, and hands back a `&mut [T]` for APIs like `Read::read`.
- A `BorrowedCursor` is append-only: a callee can't read, overwrite or
  de-initialize what the caller already filled.
- Writing in place is still possible through a few `unsafe` methods
  (`as_mut`, `advance_unchecked`, `set_init`), each with one documented
  precondition, so the unsafe surface is small and easy to audit.

## Example

```rust
use borrowed_buf::BorrowedBuf;
use core::mem::MaybeUninit;

let mut storage = [MaybeUninit::<u8>::uninit(); 16];
let mut buf = BorrowedBuf::new(&mut storage);

// Hand out a cursor: the callee can only append, never touch what's already filled.
let mut cursor = buf.unfilled();
cursor.append(b"hello");

// Write straight into the unfilled region, then record the progress.
// SAFETY: only initialized bytes are written, and the first one is written before advancing.
unsafe {
    cursor.as_mut()[0].write(b'!');
    cursor.advance_unchecked(1);
}

assert_eq!(buf.filled(), b"hello!");
assert_eq!(buf.init_len(), 6);
```

Any `Copy` element type works, e.g. decoding samples straight into uninitialized memory:

```rust
use borrowed_buf::{BorrowedBuf, BorrowedCursor};
use core::mem::MaybeUninit;

fn decode(input: &[u8], mut out: BorrowedCursor<'_, '_, i16>) {
    let n = out.capacity().min(input.len() / 2);
    // SAFETY: only initialized values are written.
    let slots = unsafe { out.as_mut() };
    for (slot, pair) in slots.iter_mut().zip(input.chunks_exact(2)).take(n) {
        slot.write(i16::from_le_bytes([pair[0], pair[1]]));
    }
    // SAFETY: the first `n` unfilled elements were just written.
    unsafe { out.advance_unchecked(n) };
}

let mut storage = [MaybeUninit::<i16>::uninit(); 4];
let mut samples = BorrowedBuf::new(&mut storage);
decode(&[1, 0, 0xff, 0xff], samples.unfilled());
assert_eq!(samples.filled(), &[1, -1]);
```

Reading from `std::io::Read` (with the `std` feature):

```rust
# #[cfg(feature = "std")] {
use borrowed_buf::{io, BorrowedBuf};
use core::mem::MaybeUninit;

let mut reader: &[u8] = b"some bytes";
let mut storage = [MaybeUninit::<u8>::uninit(); 4];
let mut buf = BorrowedBuf::new(&mut storage);

io::read_buf_exact(&mut reader, buf.unfilled()).unwrap();
assert_eq!(buf.filled(), b"some");
# }
```

## Differences from `core::io::BorrowedBuf`

| `core::io` (nightly)              | `borrowed-buf`                                   |
|-----------------------------------|--------------------------------------------------|
| `BorrowedBuf<'a>` (bytes only)    | `BorrowedBuf<'a, T = u8>` (any `T: Copy`)        |
| `BorrowedCursor<'a>`              | `BorrowedCursor<'buf, 'data, T = u8>`            |
| `cursor.ensure_init() -> &mut Self` | `cursor.ensure_init() -> &mut [T]` (`T::default()`) |
| `cursor.init_mut()`               | `cursor.init_mut()` + safe `cursor.uninit_mut()` |
| `Read::read_buf(cursor)`          | `io::read_buf(&mut reader, cursor)`              |
| `Read::read_buf_exact(cursor)`    | `io::read_buf_exact(&mut reader, cursor)`        |

## License

MIT
