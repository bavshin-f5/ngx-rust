use std::marker::PhantomData;

use foreign_types::{ForeignType, ForeignTypeRef};

use crate::ffi::{ngx_list_part_t, ngx_list_push, ngx_list_t};

foreign_types::foreign_type! {
    /// Wrapper struct for an [`ngx_list_t`]
    pub unsafe type NgxList<T: ForeignType>: Send {
        type CType = ngx_list_t;
        type PhantomData = T;
        // No cleanup required for pool-allocated structs
        fn drop = |_|();
    }
}

impl<T: ForeignType> NgxListRef<T> {
    /// Returns an iterator over the [`NgxListRef`].
    /// The iterator yields all items from start to end.
    #[inline]
    pub fn iter(&self) -> NgxListIter<'_, T> {
        NgxListIter::new(self)
    }

    /// Returns an iterator that allows modifying each value.
    /// The iterator yields all items from start to end.
    #[inline]
    pub fn iter_mut(&mut self) -> NgxListIterMut<'_, T> {
        NgxListIterMut::new(self)
    }

    /// Appends an element to the back of the [`NgxListRef`].
    /// Returns a mutable reference to the new element if allocation succeeds.
    pub fn push(&mut self) -> Option<&mut T::Ref> {
        let elt = unsafe { ngx_list_push(self.as_ptr()).cast::<<T as ForeignType>::CType>() };

        if elt.is_null() {
            return None;
        }

        Some(unsafe { T::Ref::from_ptr_mut(elt) })
    }
}

impl<T: ForeignType> AsRef<ngx_list_t> for NgxListRef<T> {
    fn as_ref(&self) -> &ngx_list_t {
        // SAFETY: `NgxListRef` must contain a valid pointer to the `ngx_list_t`
        unsafe { &*self.as_ptr() }
    }
}

impl<T: ForeignType> AsMut<ngx_list_t> for NgxListRef<T> {
    fn as_mut(&mut self) -> &mut ngx_list_t {
        // SAFETY: `NgxListRef` must contain a valid pointer to the `ngx_list_t`
        unsafe { &mut *self.as_ptr() }
    }
}

impl<'a, T: ForeignType> IntoIterator for &'a NgxListRef<T> {
    type Item = &'a T::Ref;

    type IntoIter = NgxListIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T: ForeignType> IntoIterator for &'a mut NgxListRef<T> {
    type Item = &'a mut T::Ref;

    type IntoIter = NgxListIterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// An iterator over the [`NgxListRef`]
pub struct NgxListIter<'a, T: ForeignType> {
    part: Option<&'a ngx_list_part_t>,
    i: usize,
    _p: PhantomData<T>,
}

impl<'a, T: ForeignType> NgxListIter<'a, T> {
    /// Creates a new iterator for the [`NgxListRef`]
    pub fn new(list: &'a NgxListRef<T>) -> Self {
        let inner: &ngx_list_t = list.as_ref();
        Self {
            part: Some(&inner.part),
            i: 0,
            _p: PhantomData,
        }
    }
}

impl<'a, T: ForeignType> Iterator for NgxListIter<'a, T>
where
    T::Ref: 'a,
{
    type Item = &'a T::Ref;

    fn next(&mut self) -> Option<Self::Item> {
        let mut part = self.part?;

        while self.i >= part.nelts {
            // SAFETY: in a well-formed list, part.next is either NULL or a valid ptr to a part
            self.part = unsafe { part.next.as_ref() };
            self.i = 0;
            part = self.part?;
        }

        let elts = part.elts.cast::<<T as ForeignType>::CType>();
        // SAFETY: well-formed list with `nelts > i` will have an element of a correct type at `i`
        let item = unsafe { T::Ref::from_ptr(elts.add(self.i)) };
        self.i += 1;
        Some(item)
    }
}

/// An iterator over the [`NgxListRef`] that allows modifying each value.
pub struct NgxListIterMut<'a, T: ForeignType> {
    part: Option<&'a ngx_list_part_t>,
    i: usize,
    _p: PhantomData<T>,
}

impl<'a, T: ForeignType> NgxListIterMut<'a, T> {
    /// Creates a new mutable iterator for the [`NgxListRef`]
    pub fn new(list: &'a mut NgxListRef<T>) -> Self {
        let inner: &mut ngx_list_t = list.as_mut();
        Self {
            part: Some(&inner.part),
            i: 0,
            _p: PhantomData,
        }
    }
}

impl<'a, T: ForeignType> Iterator for NgxListIterMut<'a, T>
where
    T::Ref: 'a,
{
    type Item = &'a mut T::Ref;

    fn next(&mut self) -> Option<Self::Item> {
        let mut part = self.part?;

        while self.i >= part.nelts {
            // SAFETY: in a well-formed list, part.next is either NULL or a valid ptr to a part
            self.part = unsafe { part.next.as_ref() };
            self.i = 0;
            part = self.part?;
        }

        let elts = part.elts.cast::<<T as ForeignType>::CType>();
        // SAFETY: well-formed list with `nelts > i` will have an element of a correct type at `i`
        let item = unsafe { T::Ref::from_ptr_mut(elts.add(self.i)) };
        self.i += 1;
        Some(item)
    }
}
