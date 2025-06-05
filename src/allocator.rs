use ::core::mem;
use ::core::ptr::{self, NonNull};

pub use allocator_api2::alloc::*;

#[cfg(feature = "alloc")]
pub use allocator_api2::{boxed, collections, vec};

/// Explicitly duplicate an object using the specified Allocator.
pub trait TryCloneIn: Sized {
    /// Target type, generic over an allocator.
    type Target<A: Allocator + Clone>;

    /// Attempts to copy the value using `alloc` as an underlying Allocator.
    fn try_clone_in<A: Allocator + Clone>(&self, alloc: A) -> Result<Self::Target<A>, AllocError>;
}

/// Moves `value` to the memory backed by `alloc` and returns a pointer.
///
/// This should be similar to `Box::into_raw(Box::try_new_in(value, alloc)?)`, except without
/// `alloc` requirement and intermediate steps.
///
/// # Note
///
/// The resulting pointer has no owner. The caller is responsible for destroying `T` and releasing
/// the memory.
pub fn allocate<T, A>(value: T, alloc: &A) -> Result<NonNull<T>, AllocError>
where
    A: Allocator,
{
    let layout = Layout::for_value(&value);
    let ptr: NonNull<T> = alloc.allocate(layout)?.cast();

    // SAFETY: the allocator succeeded and gave us a correctly aligned pointer to an uninitialized
    // data
    unsafe { ptr.cast::<mem::MaybeUninit<T>>().as_mut().write(value) };

    Ok(ptr)
}
///
/// Creates a `NonNull` that is dangling, but well-aligned for this alignment.
///
/// See also [::core::alloc::Layout::dangling()]
#[inline(always)]
pub(crate) const fn dangling_aligned<T>(align: usize) -> NonNull<T> {
    unsafe {
        let ptr = ptr::null_mut::<T>().byte_add(align);
        NonNull::new_unchecked(ptr)
    }
}

#[cfg(feature = "alloc")]
mod impls {
    use super::*;

    use super::boxed::Box;

    impl<T, OA> TryCloneIn for Box<T, OA>
    where
        T: TryCloneIn,
        OA: Allocator,
    {
        type Target<A: Allocator + Clone> = Box<<T as TryCloneIn>::Target<A>, A>;

        fn try_clone_in<A: Allocator + Clone>(
            &self,
            alloc: A,
        ) -> Result<Self::Target<A>, AllocError> {
            let x = self.as_ref().try_clone_in(alloc.clone())?;
            Box::try_new_in(x, alloc)
        }
    }
}
