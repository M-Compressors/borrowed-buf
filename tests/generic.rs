use borrowed_buf::BorrowedBuf;
use core::mem::MaybeUninit;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pixel {
    r: u8,
    g: u8,
    b: u8,
    a: f32,
}

#[test]
fn u32_elements() {
    let mut storage = [MaybeUninit::<u32>::uninit(); 4];
    let mut buf = BorrowedBuf::new(&mut storage);
    buf.unfilled().append(&[u32::MAX, 1]);
    let mut cursor = buf.unfilled();
    assert_eq!(cursor.capacity(), 2);
    assert_eq!(cursor.ensure_init(), &[0, 0]);
    cursor.ensure_init()[0] = 7;
    cursor.advance(1);
    assert_eq!(buf.filled(), &[u32::MAX, 1, 7]);
    assert_eq!(buf.init_len(), 4);
}

#[test]
fn f32_ensure_init_uses_default() {
    let mut storage = [MaybeUninit::<f32>::uninit(); 3];
    let mut buf = BorrowedBuf::new(&mut storage);
    assert_eq!(buf.unfilled().ensure_init(), &[0.0; 3]);
}

#[test]
fn struct_elements() {
    let px = Pixel {
        r: 1,
        g: 2,
        b: 3,
        a: 0.5,
    };
    let mut storage = [MaybeUninit::<Pixel>::uninit(); 3];
    let mut buf = BorrowedBuf::new(&mut storage);
    let mut cursor = buf.unfilled();
    cursor.append(&[px]);
    cursor.uninit_mut()[0].write(Pixel { a: 1.0, ..px });
    // SAFETY: the first unfilled element was just written.
    unsafe { cursor.advance_unchecked(1) };
    assert_eq!(cursor.written(), 2);
    assert_eq!(buf.filled(), &[px, Pixel { a: 1.0, ..px }]);
}

#[test]
fn from_init_generic_slice() {
    let mut storage = [Pixel::default(); 2];
    let mut buf = BorrowedBuf::from(&mut storage[..]);
    buf.unfilled().init_mut()[1].g = 9;
    buf.unfilled().advance(2);
    assert_eq!(buf.filled()[1].g, 9);
}

#[test]
fn zero_sized_elements() {
    let mut storage = [MaybeUninit::<()>::uninit(); 5];
    let mut buf = BorrowedBuf::new(&mut storage);
    let mut cursor = buf.unfilled();
    cursor.append(&[(), ()]);
    assert_eq!(cursor.ensure_init().len(), 3);
    cursor.advance(3);
    assert_eq!(buf.len(), 5);
    assert_eq!(buf.into_filled(), &[(); 5]);
}

#[test]
#[should_panic = "appended past the end"]
fn generic_append_overflow_panics() {
    let mut storage = [MaybeUninit::<u64>::uninit(); 1];
    let mut buf = BorrowedBuf::new(&mut storage);
    buf.unfilled().append(&[1, 2]);
}
