//! Types and utilities for working with [ngx_queue_t], an intrusive doubly-linked list.
//!
//! This module provides both the tools for interaction with the existing `ngx_queue_t` objects in
//! the NGINX, and useful high-level types built on top of the `ngx_queue_t`.
//!
//! See <https://nginx.org/en/docs/dev/development_guide.html#queue>.

use core::alloc::Layout;
use core::marker::PhantomData;
use core::mem;
use core::ptr::{self, NonNull};

use nginx_sys::{
    ngx_queue_data, ngx_queue_empty, ngx_queue_init, ngx_queue_insert_after,
    ngx_queue_insert_before, ngx_queue_remove, ngx_queue_t,
};

use crate::allocator::{AllocError, Allocator};

/// Trait for pointer conversions between the queue entry and its container.
///
/// # Safety
///
/// This trait must only be implemented on types that contain a queue link or wrappers with
/// compatible layout. The type then can be used to access elements of a raw queue type
/// [NgxQueue] linked via specified field.
///
/// If the struct can belong to several queues through multiple embedded `ngx_queue_t` fields,
/// a separate [NgxQueueEntry] implementation via wrapper type should be used for each queue.
pub unsafe trait NgxQueueEntry {
    /// Gets a container pointer from queue node.
    fn from_queue(q: NonNull<ngx_queue_t>) -> NonNull<Self>;
    /// Gets a queue node from a container reference.
    fn to_queue(&mut self) -> &mut ngx_queue_t;
}

unsafe impl NgxQueueEntry for ngx_queue_t {
    fn from_queue(q: NonNull<ngx_queue_t>) -> NonNull<Self> {
        q
    }

    fn to_queue(&mut self) -> &mut ngx_queue_t {
        self
    }
}

/// A wrapper over a raw `ngx_queue_t`, an intrusive doubly-linked list.
///
/// This wrapper is defined in terms of type `T` that embeds and can be converted from or to the
/// list entries.
///
/// Example:
/// ```rust,no_run
/// # use core::ptr::{NonNull, addr_of_mut};
/// # use nginx_sys::{ngx_event_t, ngx_posted_events, ngx_queue_data, ngx_queue_t};
/// # use ngx::collections::queue::{NgxQueue, NgxQueueEntry};
/// // We need a wrapper type to define [NgxQueueEntry] on.
/// #[repr(transparent)]
/// struct PostedEvent(ngx_event_t);
///
/// unsafe impl NgxQueueEntry for PostedEvent {
///     fn from_queue(q: NonNull<ngx_queue_t>) -> NonNull<Self> {
///         // We can safely cast obtained ngx_event_t to a transparent wrapper.
///         unsafe { ngx_queue_data!(q, ngx_event_t, queue) }.cast()
///     }
///
///     fn to_queue(&mut self) -> &mut ngx_queue_t {
///         &mut self.0.queue
///     }
/// }
///
/// // SAFETY: `ngx_posted_events` global static is a list of `ngx_event_t` linked via
/// // `ngx_event_t.queue`.
/// // NGINX is single-threaded, so we get exclusive access to the static.
/// let posted: &mut NgxQueue<PostedEvent> =
///         unsafe { NgxQueue::from_ptr_mut(addr_of_mut!(ngx_posted_events)) };
/// ```
///
/// See <https://nginx.org/en/docs/dev/development_guide.html#queue>.
#[derive(Debug)]
#[repr(transparent)]
pub struct NgxQueue<T> {
    head: ngx_queue_t,
    _type: PhantomData<T>,
}

impl<T> NgxQueue<T>
where
    T: NgxQueueEntry,
{
    /// Creates a queue reference from a pointer to [ngx_queue_t].
    ///
    /// # Safety
    ///
    /// `head` is a valid pointer to a list head, and `T::from_queue` on the list entries results in
    /// valid pointers to `T`.
    pub unsafe fn from_ptr<'a>(head: *const ngx_queue_t) -> &'a Self {
        &*head.cast()
    }

    /// Creates a mutable queue reference from a pointer to [ngx_queue_t].
    ///
    /// # Safety
    ///
    /// `head` is a valid pointer to a list head, and `T::from_queue` on the list entries results in
    /// valid pointers to `T`.
    pub unsafe fn from_ptr_mut<'a>(head: *mut ngx_queue_t) -> &'a mut Self {
        &mut *head.cast()
    }

    /// Returns `true` if the queue contains no elements.
    pub fn is_empty(&self) -> bool {
        self.head.prev.is_null() || unsafe { ngx_queue_empty(&self.head) }
    }

    /// Appends an element to the end of the queue.
    pub fn push_back(&mut self, entry: &mut T) {
        if self.head.prev.is_null() {
            unsafe { ngx_queue_init(&mut self.head) }
        }

        unsafe { ngx_queue_insert_before(&mut self.head, entry.to_queue()) }
    }

    /// Appends an element to the beginning of the queue.
    pub fn push_front(&mut self, entry: &mut T) {
        if self.head.prev.is_null() {
            unsafe { ngx_queue_init(&mut self.head) }
        }

        unsafe { ngx_queue_insert_after(&mut self.head, entry.to_queue()) }
    }

    /// Returns an iterator over the entries of the queue.
    pub fn iter(&self) -> NgxQueueIter<'_, T> {
        NgxQueueIter::new(&self.head)
    }

    /// Returns a mutable iterator over the entries of the queue.
    pub fn iter_mut(&mut self) -> NgxQueueIterMut<'_, T> {
        NgxQueueIterMut {
            head: NonNull::from(&self.head),
            current: NonNull::from(&self.head),
            _lifetime: PhantomData,
        }
    }
}

/// An iterator for the queue.
pub struct NgxQueueIter<'a, T> {
    head: NonNull<ngx_queue_t>,
    current: NonNull<ngx_queue_t>,
    _lifetime: PhantomData<&'a T>,
}

impl<'a, T> NgxQueueIter<'a, T>
where
    T: NgxQueueEntry,
{
    /// Creates a new queue iterator.
    pub fn new(head: &'a ngx_queue_t) -> Self {
        let head = NonNull::from(head);
        NgxQueueIter {
            head,
            current: head,
            _lifetime: PhantomData,
        }
    }
}

impl<'a, T> Iterator for NgxQueueIter<'a, T>
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
pub struct NgxQueueIterMut<'a, T> {
    head: NonNull<ngx_queue_t>,
    current: NonNull<ngx_queue_t>,
    _lifetime: PhantomData<&'a T>,
}

impl<'a, T> NgxQueueIterMut<'a, T>
where
    T: NgxQueueEntry,
{
    /// Creates a new mutable queue iterator.
    pub fn new(head: &'a mut ngx_queue_t) -> Self {
        let head = NonNull::from(head);
        NgxQueueIterMut {
            head,
            current: head,
            _lifetime: PhantomData,
        }
    }
}

impl<'a, T> Iterator for NgxQueueIterMut<'a, T>
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

/// A doubly-linked list that owns elements of type `T` backed by the specified allocator `A`.
#[derive(Debug)]
pub struct Queue<T, A> {
    raw: NgxQueue<QueueEntry<T>>,
    len: usize,
    alloc: A,
}

impl<T, A: Allocator + Clone> Queue<T, A> {
    /// Creates a new list with specified allocator.
    pub const fn new_in(alloc: A) -> Self {
        let raw = NgxQueue {
            head: unsafe { mem::zeroed() },
            _type: PhantomData,
        };
        Self { raw, len: 0, alloc }
    }

    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        &self.alloc
    }

    /// Returns `true` if the list contains no elements.
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Returns the number of elements in the queue.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Removes the last element and returns it or `None` if the list is empty.
    pub fn pop_back(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let node = NonNull::new(self.raw.head.prev)?;
        Some(unsafe { self.remove(node) })
    }

    /// Removes the first element and returns it or `None` if the list is empty.
    pub fn pop_front(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let node = NonNull::new(self.raw.head.next)?;
        Some(unsafe { self.remove(node) })
    }

    /// Appends an element to the end of the list.
    pub fn push_back(&mut self, item: T) -> Result<&mut T, AllocError> {
        let mut entry = QueueEntry::new_in(item, self.allocator())?;
        let entry = unsafe { entry.as_mut() };
        self.raw.push_back(entry);
        self.len += 1;
        Ok(&mut entry.item)
    }

    /// Appends an element to the beginning of the list.
    pub fn push_front(&mut self, item: T) -> Result<&mut T, AllocError> {
        let mut entry = QueueEntry::new_in(item, self.allocator())?;
        let entry = unsafe { entry.as_mut() };
        self.raw.push_front(entry);
        self.len += 1;
        Ok(&mut entry.item)
    }

    /// Removes a node from the queue and returns the contained value.
    ///
    /// # Safety
    ///
    /// `node` must be an element of this list.
    unsafe fn remove(&mut self, node: NonNull<ngx_queue_t>) -> T {
        ngx_queue_remove(node.as_ptr());
        self.len -= 1;

        let entry = QueueEntry::<T>::from_queue(node);
        let copy = entry.read();
        // Skip drop as QueueEntry is already copied to `x`.
        self.allocator()
            .deallocate(entry.cast(), Layout::for_value(entry.as_ref()));
        copy.item
    }

    /// Returns an iterator over the entries of the list.
    pub fn iter(&self) -> QueueIter<'_, T> {
        QueueIter::new(&self.raw.head)
    }

    /// Returns a mutable iterator over the entries of the list.
    pub fn iter_mut(&mut self) -> QueueIterMut<'_, T> {
        QueueIterMut::new(&mut self.raw.head)
    }
}

#[derive(Debug)]
struct QueueEntry<T> {
    queue: ngx_queue_t,
    item: T,
}

unsafe impl<T> NgxQueueEntry for QueueEntry<T> {
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

/// An iterator for the linked list [Queue].
pub struct QueueIter<'a, T>(NgxQueueIter<'a, QueueEntry<T>>);

impl<'a, T> QueueIter<'a, T> {
    /// Creates a new iterator for the linked list.
    pub fn new(head: &'a ngx_queue_t) -> Self {
        Self(NgxQueueIter::new(head))
    }
}

impl<'a, T> Iterator for QueueIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        Some(&self.0.next()?.item)
    }
}

/// A mutable iterator for the linked list [Queue].
pub struct QueueIterMut<'a, T>(NgxQueueIterMut<'a, QueueEntry<T>>);

impl<'a, T> QueueIterMut<'a, T> {
    /// Creates a new mutable iterator for the linked list.
    pub fn new(head: &'a mut ngx_queue_t) -> Self {
        Self(NgxQueueIterMut::new(head))
    }
}

impl<'a, T> Iterator for QueueIterMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        Some(&mut self.0.next()?.item)
    }
}
