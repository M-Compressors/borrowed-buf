use borrowed_buf::BorrowedBuf;
use core::fmt::Debug;
use core::mem::MaybeUninit;

fn uninit<const N: usize>() -> [MaybeUninit<u8>; N] {
    [MaybeUninit::uninit(); N]
}

#[test]
fn new_is_empty_and_uninit() {
    let mut storage = uninit::<8>();
    let mut buf = BorrowedBuf::new(&mut storage);
    assert_eq!(buf.capacity(), 8);
    assert_eq!(buf.len(), 0);
    assert!(buf.is_empty());
    assert_eq!(buf.init_len(), 0);
    assert_eq!(buf.filled(), b"");
    let mut cursor = buf.unfilled();
    assert_eq!(cursor.capacity(), 8);
    assert_eq!(cursor.init_len(), 0);
    assert!(cursor.init_mut().is_empty());
    assert_eq!(cursor.uninit_mut().len(), 8);
}

#[test]
fn from_init_slice_is_fully_initialized() {
    let mut storage = [7u8; 5];
    let mut buf = BorrowedBuf::from(&mut storage[..]);
    assert_eq!(buf.init_len(), 5);
    let mut cursor = buf.unfilled();
    assert_eq!(cursor.init_mut(), &[7; 5]);
    assert!(cursor.uninit_mut().is_empty());
    cursor.advance(3);
    assert_eq!(buf.filled(), &[7; 3]);
}

#[test]
fn from_uninit_slice_is_uninitialized() {
    let mut storage = uninit::<3>();
    let mut buf = BorrowedBuf::from(&mut storage[..]);
    assert_eq!((buf.capacity(), buf.init_len()), (3, 0));
    buf.unfilled().append(b"a");
    assert_eq!(buf.filled(), b"a");
}

#[test]
fn zero_capacity() {
    let mut buf = BorrowedBuf::<u8>::new(&mut []);
    let mut cursor = buf.unfilled();
    assert_eq!(cursor.capacity(), 0);
    assert!(cursor.ensure_init().is_empty());
    cursor.append(&[]);
    cursor.advance(0);
    assert_eq!(buf.into_filled(), b"");
}

#[test]
fn append_fills_and_initializes() {
    let mut storage = uninit::<8>();
    let mut buf = BorrowedBuf::new(&mut storage);
    buf.unfilled().append(b"abc");
    buf.unfilled().append(b"de");
    assert_eq!(buf.filled(), b"abcde");
    assert_eq!(buf.init_len(), 5);
    buf.filled_mut()[0] = b'A';
    assert_eq!(buf.into_filled(), b"Abcde");
}

#[test]
#[should_panic = "appended past the end"]
fn append_overflow_panics() {
    let mut storage = uninit::<4>();
    let mut buf = BorrowedBuf::new(&mut storage);
    buf.unfilled().append(b"12345");
}

#[test]
#[should_panic = "advanced past the initialized region"]
fn advance_past_init_panics() {
    let mut storage = uninit::<4>();
    let mut buf = BorrowedBuf::new(&mut storage);
    buf.unfilled().advance(1);
}

#[test]
fn ensure_init_zeroes_only_the_uninit_tail() {
    let mut storage = uninit::<6>();
    let mut buf = BorrowedBuf::new(&mut storage);
    buf.unfilled().append(b"xyz");
    buf.clear();
    assert_eq!(buf.init_len(), 3);

    let mut cursor = buf.unfilled();
    // The previously initialized bytes are kept, the rest is zeroed.
    assert_eq!(cursor.ensure_init(), b"xyz\0\0\0");
    assert_eq!(cursor.init_len(), 6);
    cursor.ensure_init()[..2].copy_from_slice(b"hi");
    cursor.advance(2);
    assert_eq!(buf.filled(), b"hi");
    assert_eq!(buf.init_len(), 6);
}

#[test]
fn write_uninit_then_set_init() {
    let mut storage = uninit::<8>();
    let mut buf = BorrowedBuf::new(&mut storage);
    buf.unfilled().append(b"ab");

    let mut cursor = buf.unfilled();
    for (i, byte) in cursor.uninit_mut()[..3].iter_mut().enumerate() {
        byte.write(b'0' + i as u8);
    }
    // SAFETY: the first 3 unfilled bytes were just written.
    unsafe { cursor.set_init(3) };
    assert_eq!(cursor.init_len(), 3);
    assert_eq!(cursor.init_mut(), b"012");
    cursor.advance(3);
    assert_eq!(buf.filled(), b"ab012");
}

#[test]
fn set_init_never_shrinks_and_clamps() {
    let mut storage = [0u8; 4];
    let mut buf = BorrowedBuf::from(&mut storage[..]);
    // SAFETY: trivially true, and it must not shrink `init`.
    unsafe { buf.set_init(1) };
    assert_eq!(buf.init_len(), 4);
    // SAFETY: the whole buffer is initialized; `n` is clamped to the capacity.
    unsafe { buf.set_init(100) };
    assert_eq!(buf.init_len(), 4);
    // SAFETY: as above, via the cursor.
    unsafe { buf.unfilled().set_init(100) };
    assert_eq!(buf.init_len(), 4);
}

#[test]
fn advance_unchecked_extends_init() {
    let mut storage = uninit::<4>();
    let mut buf = BorrowedBuf::new(&mut storage);
    let mut cursor = buf.unfilled();
    // SAFETY: only initialized bytes are written.
    let raw = unsafe { cursor.as_mut() };
    raw[0].write(1);
    raw[1].write(2);
    // SAFETY: the first 2 unfilled bytes were just written.
    unsafe { cursor.advance_unchecked(2) };
    assert_eq!(cursor.written(), 2);
    assert_eq!(cursor.capacity(), 2);
    assert_eq!(buf.filled(), &[1, 2]);
    assert_eq!(buf.init_len(), 2);
}

#[test]
fn reborrow_tracks_written_separately() {
    let mut storage = uninit::<8>();
    let mut buf = BorrowedBuf::new(&mut storage);
    let mut outer = buf.unfilled();
    outer.append(b"a");
    {
        let mut inner = outer.reborrow();
        assert_eq!(inner.written(), 0);
        inner.append(b"bc");
        assert_eq!(inner.written(), 2);
    }
    assert_eq!(outer.written(), 3);
    assert_eq!(outer.capacity(), 5);
    assert_eq!(buf.filled(), b"abc");
}

#[test]
fn clear_keeps_init_for_reuse() {
    let mut storage = uninit::<4>();
    let mut buf = BorrowedBuf::new(&mut storage);
    buf.unfilled().ensure_init();
    buf.clear();
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.init_len(), 4);
    buf.unfilled().advance(4);
    assert_eq!(buf.filled(), &[0; 4]);
}

#[test]
fn into_filled_outlives_the_buf() {
    let mut storage = uninit::<4>();
    let filled: &mut [u8] = {
        let mut buf = BorrowedBuf::new(&mut storage);
        buf.unfilled().append(b"ok");
        buf.into_filled()
    };
    assert_eq!(filled, b"ok");
}

#[test]
fn debug_output() {
    let mut storage = uninit::<4>();
    let mut buf = BorrowedBuf::new(&mut storage);
    buf.unfilled().append(b"a");
    assert_eq!(
        format!("{buf:?}"),
        "BorrowedBuf { filled: 1, init: 1, capacity: 4 }"
    );
    assert_eq!(
        format!("{:?}", buf.unfilled()),
        "BorrowedCursor { written: 0, init_len: 0, capacity: 3 }"
    );
}

/// Applies a random sequence of operations and checks the result against a plain `Vec` model.
/// Under Miri this catches any read of uninitialized memory or aliasing violation.
#[test]
fn random_ops_match_model() {
    random_ops(|b| b);
    random_ops(|b| u64::from(b) << 40 | 0xff);
    random_ops(|b| (f32::from(b), [b; 3]));
    random_ops(|_| ());
}

fn random_ops<T: Copy + Default + PartialEq + Debug>(elem: impl Fn(u8) -> T) {
    const CAP: usize = 32;
    let rounds = if cfg!(miri) { 4 } else { 200 };
    let mut rng = 0x2545_f491_4f6c_dd1du64;
    let mut next = |bound: usize| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng % bound as u64) as usize
    };

    for _ in 0..rounds {
        let mut storage = [MaybeUninit::<T>::uninit(); CAP];
        let mut buf = BorrowedBuf::new(&mut storage);
        // Model: the initialized contents and how many of them are filled.
        let mut init: Vec<T> = Vec::new();
        let mut filled = 0;

        for _ in 0..32 {
            let mut cursor = buf.unfilled();
            let room = CAP - filled;
            match next(7) {
                0 => {
                    let data: Vec<T> = (0..next(room + 1)).map(|_| elem(next(256) as u8)).collect();
                    cursor.append(&data);
                    let end = filled + data.len();
                    if init.len() < end {
                        init.resize(end, T::default());
                    }
                    init[filled..end].copy_from_slice(&data);
                    filled = end;
                }
                1 => {
                    let n = next(init.len() - filled + 1);
                    cursor.advance(n);
                    filled += n;
                }
                2 => {
                    let tail = cursor.ensure_init();
                    assert_eq!(&tail[..init.len() - filled], &init[filled..]);
                    init.resize(CAP, T::default());
                }
                3 => {
                    let n = next(room + 1);
                    let value = elem(next(256) as u8);
                    // Write the uninit tail, then also overwrite the init part through as_mut.
                    for b in cursor.uninit_mut().iter_mut().take(n) {
                        b.write(value);
                    }
                    // SAFETY: only initialized bytes are written.
                    for b in unsafe { cursor.as_mut() }.iter_mut().take(n) {
                        b.write(value);
                    }
                    // SAFETY: the first `n` unfilled bytes were just written.
                    unsafe { cursor.advance_unchecked(n) };
                    let end = filled + n;
                    if init.len() < end {
                        init.resize(end, T::default());
                    }
                    init[filled..end].fill(value);
                    filled = end;
                }
                4 => {
                    let value = elem(next(256) as u8);
                    cursor.init_mut().fill(value);
                    init[filled..].fill(value);
                }
                5 => {
                    let mut inner = cursor.reborrow();
                    let n = next(room + 1);
                    inner.append(&seq(n, &elem));
                    assert_eq!(inner.written(), n);
                    let end = filled + n;
                    if init.len() < end {
                        init.resize(end, T::default());
                    }
                    init[filled..end].copy_from_slice(&seq(n, &elem));
                    filled = end;
                }
                _ => {
                    buf.clear();
                    filled = 0;
                }
            }

            assert_eq!(buf.len(), filled);
            assert_eq!(buf.init_len(), init.len());
            assert_eq!(buf.filled(), &init[..filled]);
            assert_eq!(buf.unfilled().init_mut(), &init[filled..]);
        }
    }
}

fn seq<T>(n: usize, elem: impl Fn(u8) -> T) -> Vec<T> {
    (0..n as u8).map(elem).collect()
}
