#![doc = include_str!("../README.md")]
#![no_std]
#![deny(missing_docs, unsafe_op_in_unsafe_fn)]

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
pub mod io;

use core::fmt;
use core::hint::assert_unchecked;
use core::mem::MaybeUninit;
use core::ptr;

/// A borrowed buffer that is filled incrementally and tracks how much of it is initialized.
///
/// The buffer is split into three regions:
///
/// ```text
/// [             capacity              ]
/// [ filled |         unfilled         ]
/// [    initialized    | uninitialized ]
/// ```
///
/// The initialized region only ever grows, so re-using a buffer (see [`clear`](Self::clear))
/// never pays for initializing the same memory twice.
///
/// Elements are `u8` by default. Any `T: Copy` works: the buffer only borrows its memory and never
/// drops elements, which `Copy` makes trivially correct.
///
/// # Examples
///
/// ```
/// use borrowed_buf::BorrowedBuf;
/// use core::mem::MaybeUninit;
///
/// let mut storage = [MaybeUninit::<u8>::uninit(); 8];
/// let mut buf = BorrowedBuf::new(&mut storage);
///
/// buf.unfilled().append(b"abc");
/// assert_eq!(buf.filled(), b"abc");
///
/// // Reuse the buffer: the first 3 bytes stay initialized.
/// buf.clear();
/// assert_eq!((buf.len(), buf.init_len()), (0, 3));
/// ```
pub struct BorrowedBuf<'data, T = u8> {
    buf: &'data mut [MaybeUninit<T>],
    // Invariant: `filled <= init <= buf.len()`, and `buf[..init]` is initialized.
    filled: usize,
    init: usize,
}

impl<'data, T: Copy> BorrowedBuf<'data, T> {
    /// Creates a buffer over possibly uninitialized memory. Nothing is considered initialized.
    #[inline]
    pub const fn new(buf: &'data mut [MaybeUninit<T>]) -> Self {
        BorrowedBuf {
            buf,
            filled: 0,
            init: 0,
        }
    }

    /// Creates a buffer over initialized memory. The whole buffer is considered initialized.
    ///
    /// ```
    /// use borrowed_buf::BorrowedBuf;
    ///
    /// let mut storage = [0u16; 4];
    /// let mut buf = BorrowedBuf::from_init(&mut storage);
    /// assert_eq!(buf.init_len(), 4);
    ///
    /// // No initialization needed before advancing.
    /// buf.unfilled().advance(2);
    /// assert_eq!(buf.filled(), &[0, 0]);
    /// ```
    #[inline]
    pub const fn from_init(buf: &'data mut [T]) -> Self {
        let init = buf.len();
        // SAFETY: `T` and `MaybeUninit<T>` have the same layout. Uninitialized values can never
        // be written into `buf[..init]` through safe code, so the original slice stays valid.
        let buf = unsafe { &mut *(buf as *mut [T] as *mut [MaybeUninit<T>]) };
        BorrowedBuf {
            buf,
            filled: 0,
            init,
        }
    }

    /// Hints the optimizer about the struct invariant, which removes bounds checks downstream.
    #[inline(always)]
    const fn check(&self) {
        // SAFETY: upheld by every method that mutates `filled` or `init`.
        unsafe { assert_unchecked(self.filled <= self.init && self.init <= self.buf.len()) }
    }

    /// Total size of the buffer, in elements.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Length of the filled region.
    #[inline]
    pub const fn len(&self) -> usize {
        self.filled
    }

    /// Returns `true` if nothing has been filled yet.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// Length of the initialized region (always `>= len()`).
    #[inline]
    pub const fn init_len(&self) -> usize {
        self.init
    }

    /// The filled region.
    #[inline]
    pub fn filled(&self) -> &[T] {
        self.check();
        // SAFETY: `buf[..filled]` is initialized.
        unsafe { assume_init(&self.buf[..self.filled]) }
    }

    /// The filled region, mutably.
    #[inline]
    pub fn filled_mut(&mut self) -> &mut [T] {
        self.check();
        // SAFETY: `buf[..filled]` is initialized.
        unsafe { assume_init_mut(&mut self.buf[..self.filled]) }
    }

    /// Consumes the buffer, returning the filled region with the original lifetime.
    ///
    /// ```
    /// use borrowed_buf::BorrowedBuf;
    /// use core::mem::MaybeUninit;
    ///
    /// fn fill(storage: &mut [MaybeUninit<u8>]) -> &mut [u8] {
    ///     let mut buf = BorrowedBuf::new(storage);
    ///     buf.unfilled().append(b"done");
    ///     buf.into_filled()
    /// }
    ///
    /// let mut storage = [MaybeUninit::uninit(); 8];
    /// assert_eq!(fill(&mut storage), b"done");
    /// ```
    #[inline]
    pub fn into_filled(self) -> &'data mut [T] {
        self.check();
        // SAFETY: `buf[..filled]` is initialized.
        unsafe { assume_init_mut(&mut self.buf[..self.filled]) }
    }

    /// Returns a cursor over the unfilled region, used to append data.
    #[inline]
    pub fn unfilled<'buf>(&'buf mut self) -> BorrowedCursor<'buf, 'data, T> {
        BorrowedCursor {
            start: self.filled,
            buf: self,
        }
    }

    /// Resets the filled region to empty. The initialized region is kept.
    #[inline]
    pub fn clear(&mut self) -> &mut Self {
        self.filled = 0;
        self
    }

    /// Marks the first `n` elements of the buffer as initialized.
    ///
    /// Never shrinks the initialized region, and `n` is clamped to the capacity.
    ///
    /// # Safety
    ///
    /// The first `n` elements of the buffer must be initialized.
    #[inline]
    pub unsafe fn set_init(&mut self, n: usize) -> &mut Self {
        self.init = self.init.max(n.min(self.buf.len()));
        self
    }
}

impl<'data, T: Copy> From<&'data mut [MaybeUninit<T>]> for BorrowedBuf<'data, T> {
    #[inline]
    fn from(buf: &'data mut [MaybeUninit<T>]) -> Self {
        Self::new(buf)
    }
}

impl<'data, T: Copy> From<&'data mut [T]> for BorrowedBuf<'data, T> {
    #[inline]
    fn from(buf: &'data mut [T]) -> Self {
        Self::from_init(buf)
    }
}

impl<T> fmt::Debug for BorrowedBuf<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BorrowedBuf")
            .field("filled", &self.filled)
            .field("init", &self.init)
            .field("capacity", &self.buf.len())
            .finish()
    }
}

/// A writeable view of the unfilled region of a [`BorrowedBuf`].
///
/// A cursor can only append: data that has been filled can't be overwritten or un-filled through
/// it, so a callee handed a cursor can't disturb what the caller already has.
///
/// # Examples
///
/// ```
/// use borrowed_buf::{BorrowedBuf, BorrowedCursor};
/// use core::mem::MaybeUninit;
///
/// // A producer that only sees the free space.
/// fn produce(mut cursor: BorrowedCursor<'_, '_, u32>) {
///     let n = cursor.capacity().min(3);
///     // SAFETY: only initialized values are written.
///     let out = unsafe { cursor.as_mut() };
///     for (i, slot) in out[..n].iter_mut().enumerate() {
///         slot.write(i as u32 * 10);
///     }
///     // SAFETY: the first `n` unfilled elements were just written.
///     unsafe { cursor.advance_unchecked(n) };
/// }
///
/// let mut storage = [MaybeUninit::uninit(); 4];
/// let mut buf = BorrowedBuf::new(&mut storage);
/// buf.unfilled().append(&[7]);
/// produce(buf.unfilled());
/// assert_eq!(buf.filled(), &[7, 0, 10, 20]);
/// ```
pub struct BorrowedCursor<'buf, 'data, T = u8> {
    buf: &'buf mut BorrowedBuf<'data, T>,
    // Value of `buf.filled` when this cursor was created.
    start: usize,
}

impl<'data, T: Copy> BorrowedCursor<'_, 'data, T> {
    /// Returns a shorter-lived cursor over the same region, e.g. to pass to a callee by value.
    ///
    /// [`written`](Self::written) on the new cursor starts at zero.
    ///
    /// ```
    /// use borrowed_buf::BorrowedBuf;
    /// use core::mem::MaybeUninit;
    ///
    /// let mut storage = [MaybeUninit::uninit(); 8];
    /// let mut buf = BorrowedBuf::new(&mut storage);
    /// let mut cursor = buf.unfilled();
    /// for chunk in [&b"ab"[..], b"cde"] {
    ///     let mut sub = cursor.reborrow();
    ///     sub.append(chunk);
    ///     assert_eq!(sub.written(), chunk.len());
    /// }
    /// assert_eq!(cursor.written(), 5);
    /// ```
    #[inline]
    pub fn reborrow(&mut self) -> BorrowedCursor<'_, 'data, T> {
        BorrowedCursor {
            start: self.buf.filled,
            buf: self.buf,
        }
    }

    /// Number of elements that can still be appended.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.buf.check();
        self.buf.buf.len() - self.buf.filled
    }

    /// Number of elements appended through this cursor (and its reborrows).
    #[inline]
    pub const fn written(&self) -> usize {
        self.buf.filled - self.start
    }

    /// Length of the initialized part of the unfilled region.
    #[inline]
    pub const fn init_len(&self) -> usize {
        self.buf.check();
        self.buf.init - self.buf.filled
    }

    /// The initialized part of the unfilled region.
    #[inline]
    pub fn init_mut(&mut self) -> &mut [T] {
        let b = &mut *self.buf;
        b.check();
        // SAFETY: `buf[filled..init]` is initialized.
        unsafe { assume_init_mut(&mut b.buf[b.filled..b.init]) }
    }

    /// The uninitialized part of the unfilled region.
    ///
    /// Note that it starts [`init_len`](Self::init_len) elements into the unfilled region. After
    /// writing `k` elements to it, record that with `set_init(init_len + k)`.
    ///
    /// ```
    /// use borrowed_buf::BorrowedBuf;
    /// use core::mem::MaybeUninit;
    ///
    /// let mut storage = [MaybeUninit::uninit(); 4];
    /// let mut buf = BorrowedBuf::new(&mut storage);
    /// let mut cursor = buf.unfilled();
    /// let init_len = cursor.init_len();
    /// cursor.uninit_mut()[0].write(b'x');
    /// // SAFETY: everything up to and including the byte just written is initialized.
    /// unsafe { cursor.set_init(init_len + 1) };
    /// assert_eq!(cursor.init_mut(), b"x");
    /// ```
    #[inline]
    pub fn uninit_mut(&mut self) -> &mut [MaybeUninit<T>] {
        let b = &mut *self.buf;
        b.check();
        &mut b.buf[b.init..]
    }

    /// The whole unfilled region, initialized or not.
    ///
    /// # Safety
    ///
    /// The caller must not write uninitialized values into the slice: its first
    /// [`init_len`](Self::init_len) elements are already tracked as initialized.
    #[inline]
    pub unsafe fn as_mut(&mut self) -> &mut [MaybeUninit<T>] {
        let b = &mut *self.buf;
        b.check();
        &mut b.buf[b.filled..]
    }

    /// Initializes the uninitialized part of the unfilled region with `T::default()` (zero for
    /// numbers) and returns the whole unfilled region.
    ///
    /// Memory is initialized at most once over the lifetime of the [`BorrowedBuf`].
    ///
    /// ```
    /// use borrowed_buf::BorrowedBuf;
    /// use core::mem::MaybeUninit;
    ///
    /// let mut storage = [MaybeUninit::<f32>::uninit(); 4];
    /// let mut buf = BorrowedBuf::new(&mut storage);
    /// let mut cursor = buf.unfilled();
    ///
    /// let out = cursor.ensure_init();
    /// assert_eq!(out, &[0.0; 4]);
    /// out[0] = 1.5;
    /// cursor.advance(1);
    /// assert_eq!(buf.filled(), &[1.5]);
    /// ```
    #[inline]
    pub fn ensure_init(&mut self) -> &mut [T]
    where
        T: Default,
    {
        let b = &mut *self.buf;
        b.check();
        // SAFETY: `init <= buf.len()` by the invariant.
        unsafe { b.buf.get_unchecked_mut(b.init..) }.fill(MaybeUninit::new(T::default()));
        b.init = b.buf.len();
        // SAFETY: `filled <= buf.len()` by the invariant, and the whole buffer is initialized now.
        unsafe { assume_init_mut(b.buf.get_unchecked_mut(b.filled..)) }
    }

    /// Marks the first `n` elements of the unfilled region as filled.
    ///
    /// # Panics
    ///
    /// Panics if `n > self.init_len()`.
    #[inline]
    #[track_caller]
    pub fn advance(&mut self, n: usize) -> &mut Self {
        assert!(n <= self.init_len(), "advanced past the initialized region");
        self.buf.filled += n;
        self
    }

    /// Marks the first `n` elements of the unfilled region as filled (and thus initialized).
    ///
    /// # Safety
    ///
    /// The first `n` elements of the unfilled region must be initialized, which implies
    /// `n <= self.capacity()`.
    #[inline]
    pub unsafe fn advance_unchecked(&mut self, n: usize) -> &mut Self {
        let b = &mut *self.buf;
        b.filled += n;
        b.init = b.init.max(b.filled);
        self
    }

    /// Marks the first `n` elements of the unfilled region as initialized.
    ///
    /// Never shrinks the initialized region, and `n` is clamped to the capacity.
    ///
    /// # Safety
    ///
    /// The first `n` elements of the unfilled region must be initialized.
    #[inline]
    pub unsafe fn set_init(&mut self, n: usize) -> &mut Self {
        let b = &mut *self.buf;
        b.init = b.init.max(b.filled + n.min(b.buf.len() - b.filled));
        self
    }

    /// Appends `data` to the filled region.
    ///
    /// # Panics
    ///
    /// Panics if `data.len() > self.capacity()`.
    ///
    /// ```
    /// use borrowed_buf::BorrowedBuf;
    /// use core::mem::MaybeUninit;
    ///
    /// let mut storage = [MaybeUninit::uninit(); 4];
    /// let mut buf = BorrowedBuf::new(&mut storage);
    /// buf.unfilled().append(&[1u64, 2]);
    /// buf.unfilled().append(&[3]);
    /// assert_eq!(buf.filled(), &[1, 2, 3]);
    /// ```
    #[inline]
    #[track_caller]
    pub fn append(&mut self, data: &[T]) {
        assert!(
            data.len() <= self.capacity(),
            "appended past the end of the buffer"
        );
        let b = &mut *self.buf;
        // SAFETY: `filled + data.len() <= buf.len()` was just checked, and `data` can't overlap
        // `buf` because the latter is borrowed mutably.
        unsafe {
            let dst = b.buf.as_mut_ptr().add(b.filled).cast::<T>();
            ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        }
        b.filled += data.len();
        b.init = b.init.max(b.filled);
    }
}

impl<T> fmt::Debug for BorrowedCursor<'_, '_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &*self.buf;
        f.debug_struct("BorrowedCursor")
            .field("written", &(b.filled - self.start))
            .field("init_len", &(b.init - b.filled))
            .field("capacity", &(b.buf.len() - b.filled))
            .finish()
    }
}

#[inline(always)]
const unsafe fn assume_init<T>(s: &[MaybeUninit<T>]) -> &[T] {
    // SAFETY: the caller guarantees `s` is initialized; the layouts are identical.
    unsafe { &*(s as *const [MaybeUninit<T>] as *const [T]) }
}

#[inline(always)]
const unsafe fn assume_init_mut<T>(s: &mut [MaybeUninit<T>]) -> &mut [T] {
    // SAFETY: the caller guarantees `s` is initialized; the layouts are identical.
    unsafe { &mut *(s as *mut [MaybeUninit<T>] as *mut [T]) }
}
