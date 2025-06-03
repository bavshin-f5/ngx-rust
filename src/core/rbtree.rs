//! Wrapper over the `ngx_rbtree_t`.

use core::alloc::Layout;
use core::cmp::Ordering;
#[allow(deprecated)]
use core::hash::{Hash, Hasher, SipHasher};
use core::marker::PhantomData;
use core::ptr::{self, NonNull};
use core::{borrow, mem};

use nginx_sys::{
    ngx_rbtree_data, ngx_rbtree_delete, ngx_rbtree_init, ngx_rbtree_insert,
    ngx_rbtree_insert_value, ngx_rbtree_key_t, ngx_rbtree_min, ngx_rbtree_next, ngx_rbtree_node_t,
    ngx_rbtree_t,
};

use crate::allocator::{self, AllocError, Allocator};

/// Wrapper over the `ngx_rbtree_t`.
#[derive(Debug)]
pub struct RbTree<K, V, A>
where
    A: Allocator,
{
    tree: ngx_rbtree_t,
    sentinel: NonNull<ngx_rbtree_node_t>,
    alloc: A,
    _ph: PhantomData<(K, V)>,
}

struct Node<K, V> {
    node: ngx_rbtree_node_t,
    key: K,
    value: V,
}

impl<K, V> Node<K, V>
where
    K: Hash,
{
    fn new(key: K, value: V) -> Self {
        #[allow(deprecated)]
        let mut h = SipHasher::new();
        key.hash(&mut h);
        let hash = h.finish() as ngx_rbtree_key_t;

        let mut node: ngx_rbtree_node_t = unsafe { mem::zeroed() };
        node.key = hash;

        Self { node, key, value }
    }

    fn into_kv(self) -> (K, V) {
        (self.key, self.value)
    }
}

/// Raw iterator over the `ngx_rbtree_t` nodes.
pub struct RawIter<'a> {
    tree: &'a ngx_rbtree_t,
    node: *mut ngx_rbtree_node_t,
}

impl<'a> RawIter<'a> {
    /// Creates an iterator for the `ngx_rbtree_t`.
    pub fn new(tree: &'a ngx_rbtree_t) -> Self {
        let node = if ptr::addr_eq(tree.root, tree.sentinel) {
            // empty tree
            ptr::null_mut()
        } else {
            unsafe { ngx_rbtree_min(tree.root, tree.sentinel) }
        };

        Self { tree, node }
    }
}

impl<'a> Iterator for RawIter<'a> {
    type Item = NonNull<ngx_rbtree_node_t>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = NonNull::new(self.node)?;

        self.node = unsafe { ngx_rbtree_next(ptr::from_ref(self.tree).cast_mut(), self.node) };

        Some(item)
    }
}

/// An iterator for the [RbTree].
pub struct Iter<'a, K: 'a, V: 'a>(RawIter<'a>, PhantomData<(K, V)>);

impl<'a, K: 'a, V: 'a> Iter<'a, K, V> {
    /// Creates an iterator for the [RbTree].
    pub fn new<A: Allocator>(tree: &'a RbTree<K, V, A>) -> Self {
        Self(RawIter::new(&tree.tree), Default::default())
    }
}

impl<'a, K: 'a, V: 'a> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.0.next()?;
        // TODO: Use NonNull directly with msrv >= 1.80
        let item = unsafe { &*ngx_rbtree_data!(item.as_ptr(), Node<K, V>, node) };
        Some((&item.key, &item.value))
    }
}

/// A mutable iterator for the [RbTree].
pub struct IterMut<'a, K: 'a, V: 'a>(RawIter<'a>, PhantomData<(K, V)>);

impl<'a, K: 'a, V: 'a> IterMut<'a, K, V> {
    /// Creates an iterator for the [RbTree].
    pub fn new<A: Allocator>(tree: &'a mut RbTree<K, V, A>) -> Self {
        Self(RawIter::new(&tree.tree), Default::default())
    }
}

impl<'a, K: 'a, V: 'a> Iterator for IterMut<'a, K, V> {
    type Item = (&'a K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.0.next()?;
        // TODO: Use NonNull directly with msrv >= 1.80
        let item = unsafe { &mut *ngx_rbtree_data!(item.as_ptr(), Node<K, V>, node) };
        Some((&item.key, &mut item.value))
    }
}

impl<K, V, A> RbTree<K, V, A>
where
    A: Allocator,
{
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        &self.alloc
    }

    /// Attempts to create and initialize a new RbTree with specified allocator.
    pub fn try_new_in(alloc: A) -> Result<Self, AllocError> {
        let layout = Layout::new::<ngx_rbtree_node_t>();
        let sentinel: NonNull<ngx_rbtree_node_t> = alloc.allocate_zeroed(layout)?.cast();

        let mut this = RbTree {
            tree: unsafe { mem::zeroed() },
            sentinel,
            alloc,
            _ph: Default::default(),
        };

        unsafe {
            ngx_rbtree_init(
                &mut this.tree,
                this.sentinel.as_ptr(),
                Some(ngx_rbtree_insert_value),
            )
        };

        Ok(this)
    }

    /// Clears the tree, removing all elements.
    pub fn clear(&mut self) {
        unsafe {
            let mut p = ngx_rbtree_min(self.tree.root, self.tree.sentinel);

            while !p.is_null() {
                let mut node = NonNull::new_unchecked(ngx_rbtree_data!(p, Node<K, V>, node));
                p = ngx_rbtree_next(&mut self.tree, p);
                let layout = Layout::for_value(node.as_ref());

                ngx_rbtree_delete(&mut self.tree, &mut node.as_mut().node);
                ptr::drop_in_place(node.as_mut());
                self.allocator().deallocate(node.cast(), layout)
            }
        }
    }

    /// Returns true if the tree contains no entries.
    pub fn is_empty(&self) -> bool {
        ptr::addr_eq(self.tree.root, self.tree.sentinel)
    }

    /// Returns an iterator over the entries of the tree.
    #[inline]
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter::new(self)
    }

    /// Returns a mutable iterator over the entries of the tree.
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
        IterMut::new(self)
    }
}

impl<K, V, A> RbTree<K, V, A>
where
    A: Allocator,
    K: Hash + Ord,
{
    /// Returns a reference to the value corresponding to the key.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: borrow::Borrow<Q>,
        Q: Hash + Ord + ?Sized,
    {
        self.lookup(key).map(|x| unsafe { &x.as_ref().value })
    }

    /// Returns a mutable reference to the value corresponding to the key.
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: borrow::Borrow<Q>,
        Q: Hash + Ord + ?Sized,
    {
        self.lookup(key)
            .map(|mut x| unsafe { &mut x.as_mut().value })
    }

    /// Removes a key from the tree, returning the value at the key if the key was previously in the
    /// tree.
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: borrow::Borrow<Q>,
        Q: Hash + Ord + ?Sized,
    {
        self.remove_entry(key).map(|(_, v)| v)
    }

    /// Removes a key from the tree, returning the stored key and value if the key was previously in
    /// the tree.
    pub fn remove_entry<Q>(&mut self, key: &Q) -> Option<(K, V)>
    where
        K: borrow::Borrow<Q>,
        Q: Hash + Ord + ?Sized,
    {
        let mut node = self.lookup(key)?;
        unsafe {
            ngx_rbtree_delete(&mut self.tree, &mut node.as_mut().node);
            let layout = Layout::for_value(node.as_ref());
            // SAFETY: we make a bitwise copy of the node and dispose of the original value without
            // dropping it.
            let copy = node.as_ptr().read();
            self.allocator().deallocate(node.cast(), layout);
            Some(copy.into_kv())
        }
    }

    /// Attempts to insert a new element into the tree.
    pub fn try_insert(&mut self, key: K, value: V) -> Result<&mut V, AllocError> {
        let mut node = if let Some(mut node) = self.lookup(&key) {
            unsafe { node.as_mut().value = value };
            node
        } else {
            let node = Node::new(key, value);
            let mut node = allocator::allocate(node, self.allocator())?;
            unsafe { ngx_rbtree_insert(&mut self.tree, &mut node.as_mut().node) };
            node
        };

        Ok(unsafe { &mut node.as_mut().value })
    }

    fn lookup<Q>(&self, key: &Q) -> Option<NonNull<Node<K, V>>>
    where
        K: borrow::Borrow<Q>,
        Q: Hash + Ord + ?Sized,
    {
        #[allow(deprecated)]
        let mut h = SipHasher::new();
        key.hash(&mut h);
        let hash = h.finish() as ngx_rbtree_key_t;

        let mut node = self.tree.root;

        while !ptr::addr_eq(node, self.tree.sentinel) {
            let k = unsafe { (*node).key };

            if hash != k {
                node = if hash < k {
                    (unsafe { *node }).left
                } else {
                    (unsafe { *node }).right
                };

                continue;
            }

            let n = unsafe { ngx_rbtree_data!(node, Node<K, V>, node) };

            match Ord::cmp(unsafe { (*n).key.borrow() }, key) {
                Ordering::Less => {
                    node = unsafe { (*node).left };
                    continue;
                }
                Ordering::Equal => return Some(unsafe { NonNull::new_unchecked(n) }),
                Ordering::Greater => {
                    node = unsafe { (*node).left };
                    continue;
                }
            }
        }

        None
    }
}

impl<K, V, A> Drop for RbTree<K, V, A>
where
    A: Allocator,
{
    fn drop(&mut self) {
        self.clear();

        unsafe {
            self.allocator().deallocate(
                self.sentinel.cast(),
                Layout::for_value(self.sentinel.as_ref()),
            )
        };
    }
}
