//! A wrapper over the `ngx_queue_t`, an intrusive doubly-linked list.
use core::ptr::NonNull;
use core::{alloc::Layout, marker::PhantomData};
use core::{mem, ptr};

use nginx_sys::{
    ngx_queue_data, ngx_queue_empty, ngx_queue_init, ngx_queue_insert_after,
    ngx_queue_insert_before, ngx_queue_t,
};

use crate::allocator::{AllocError, Allocator};

/// An owning double-linked list.
#[derive(Debug)]
pub struct Queue<T, A> {
    raw: NgxQueue<QueueEntry<T>>,
    alloc: A,
}

impl<T, A: Allocator + Clone> Queue<T, A> {
    /// Creates a new Queue with specified allocator.
    pub fn new_in(alloc: A) -> Self {
        let raw = NgxQueue::default();
        Self { raw, alloc }
    }

    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        &self.alloc
    }

    /// Returns `true` if the queue contains no elements.
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Appends an element to the end of the queue.
    pub fn try_push_back(&mut self, item: T) -> Result<&mut T, AllocError> {
        let mut entry = QueueEntry::new_in(item, self.allocator())?;
        let entry = unsafe { entry.as_mut() };
        self.raw.push_back(entry);
        Ok(&mut entry.item)
    }

    /// Appends an element to the beginning of the queue.
    pub fn try_push_front(&mut self, item: T) -> Result<&mut T, AllocError> {
        let mut entry = QueueEntry::new_in(item, self.allocator())?;
        let entry = unsafe { entry.as_mut() };
        self.raw.push_front(entry);
        Ok(&mut entry.item)
    }

    /// Returns an iterator over the entries of the queue.
    pub fn iter(&self) -> impl Iterator<Item = &'_ T> {
        self.raw.iter().map(|x| &x.item)
    }

    /// Returns a mutable iterator over the entries of the queue.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &'_ mut T> {
        self.raw.iter_mut().map(|x| &mut x.item)
    }
}

#[derive(Debug)]
struct QueueEntry<T> {
    queue: ngx_queue_t,
    item: T,
}

impl<T> NgxQueueEntry for QueueEntry<T> {
    fn from_queue(q: NonNull<ngx_queue_t>) -> NonNull<Self> {
        unsafe { ngx_queue_data!(q, Self, queue) }
    }

    fn to_queue(&mut self) -> &mut ngx_queue_t {
        &mut self.queue
    }
}

impl<T> QueueEntry<T> {
    pub fn new_in(item: T, alloc: &impl Allocator) -> Result<NonNull<Self>, AllocError> {
        let p: NonNull<Self> = alloc.allocate(Layout::new::<Self>())?.cast();

        unsafe {
            let u = p.cast::<mem::MaybeUninit<Self>>().as_mut();
            // does not read the uninitialized data
            ngx_queue_init(&mut u.assume_init_mut().queue);
            ptr::write(&mut u.assume_init_mut().item, item);
        }

        Ok(p)
    }
}

/// A wrapper over the `ngx_queue_t`, an intrusive doubly-linked list.
///
/// See <https://nginx.org/en/docs/dev/development_guide.html#queue>.
#[derive(Debug)]
#[repr(transparent)]
pub struct NgxQueue<T>(ngx_queue_t, PhantomData<T>);

impl<T> NgxQueue<T>
where
    T: NgxQueueEntry,
{
    /// Creates a queue reference from a pointer to [ngx_queue_t].
    pub fn from_ptr<'a>(head: *const ngx_queue_t) -> &'a Self {
        unsafe { &*head.cast() }
    }

    /// Creates a mutable queue reference from a pointer to [ngx_queue_t].
    pub fn from_ptr_mut<'a>(head: *mut ngx_queue_t) -> &'a mut Self {
        unsafe { &mut *head.cast() }
    }

    /// Returns `true` if the queue contains no elements.
    pub fn is_empty(&self) -> bool {
        self.0.next.is_null() || unsafe { ngx_queue_empty(&self.0) }
    }

    /// Appends an element to the end of the queue.
    pub fn push_back(&mut self, entry: &mut T) {
        if self.0.next.is_null() {
            unsafe { ngx_queue_init(&mut self.0) }
        }

        unsafe { ngx_queue_insert_before(&mut self.0, entry.to_queue()) }
    }

    /// Appends an element to the beginning of the queue.
    pub fn push_front(&mut self, entry: &mut T) {
        if self.0.next.is_null() {
            unsafe { ngx_queue_init(&mut self.0) }
        }

        unsafe { ngx_queue_insert_after(&mut self.0, entry.to_queue()) }
    }

    /// Returns an iterator over the entries of the queue.
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            head: NonNull::from(&self.0),
            current: NonNull::from(&self.0),
            _pd: Default::default(),
        }
    }

    /// Returns a mutable iterator over the entries of the queue.
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            head: NonNull::from(&self.0),
            current: NonNull::from(&self.0),
            _pd: Default::default(),
        }
    }
}

impl<T> Default for NgxQueue<T> {
    fn default() -> Self {
        Self(Default::default(), Default::default())
    }
}

/// Trait for pointer conversions between the queue entry and its container.
pub trait NgxQueueEntry {
    /// Gets a container pointer from queue node.
    fn from_queue(q: NonNull<ngx_queue_t>) -> NonNull<Self>;
    /// Gets a queue node from a container reference.
    fn to_queue(&mut self) -> &mut ngx_queue_t;
}

/// An iterator for the queue.
pub struct Iter<'a, T> {
    head: NonNull<ngx_queue_t>,
    current: NonNull<ngx_queue_t>,
    _pd: PhantomData<&'a T>,
}

impl<'a, T> Iterator for Iter<'a, T>
where
    T: NgxQueueEntry + 'a,
{
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let next = NonNull::new(self.current.as_ref().next)?;
            if next == self.head {
                return None;
            }

            self.current = next;
            Some(T::from_queue(self.current).as_ref())
        }
    }
}

/// A mutable iterator for the queue.
pub struct IterMut<'a, T> {
    head: NonNull<ngx_queue_t>,
    current: NonNull<ngx_queue_t>,
    _pd: PhantomData<&'a T>,
}

impl<'a, T> Iterator for IterMut<'a, T>
where
    T: NgxQueueEntry + 'a,
{
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let next = NonNull::new(self.current.as_ref().next)?;
            if next == self.head {
                return None;
            }

            self.current = next;
            Some(T::from_queue(self.current).as_mut())
        }
    }
}
