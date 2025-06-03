//! Wrapper over the `ngx_rbtree_t`.

use core::alloc::Layout;
use core::cmp::Ordering;
#[allow(deprecated)]
use core::hash::{Hash, Hasher, SipHasher};
use core::marker::PhantomData;
use core::mem;
use core::ptr::{self, NonNull};

use nginx_sys::{
    ngx_rbtree_data, ngx_rbtree_init, ngx_rbtree_insert, ngx_rbtree_key_t, ngx_rbtree_node_t,
    ngx_rbtree_t,
};

use crate::allocator::{self, AllocError, Allocator};

/// Wrapper over the `ngx_rbtree_t`.
#[derive(Debug)]
pub struct RbTree<K, V, A> {
    tree: ngx_rbtree_t,
    sentinel: NonNull<ngx_rbtree_node_t>,
    alloc: A,
    // Magic line for dropck (Nomicon 3.9, 3.10)
    _ph: PhantomData<(K, V)>,
}

struct RbTreeNode<K, V> {
    node: ngx_rbtree_node_t,
    key: K,
    value: V,
}

impl<K, V> RbTreeNode<K, V>
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
}

impl<K, V, A> RbTree<K, V, A>
where
    K: Hash + Ord,
    A: Allocator + Clone,
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
                Some(nginx_sys::ngx_rbtree_insert_value),
            )
        };

        Ok(this)
    }

    /// Attempts to insert a new element into the tree.
    pub fn try_insert(&mut self, key: K, value: V) -> Result<&mut V, AllocError> {
        let mut node = if let Some(mut node) = self.lookup(&key) {
            unsafe { node.as_mut().value = value };
            node
        } else {
            let node = RbTreeNode::new(key, value);
            let mut node = allocator::allocate(node, self.allocator())?;
            unsafe { ngx_rbtree_insert(&mut self.tree, &mut node.as_mut().node) };
            node
        };

        Ok(unsafe { &mut node.as_mut().value })
    }

    /// Returns a reference to the value corresponding to the key.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        Q: Hash + Ord + ?Sized,
        K: core::borrow::Borrow<Q>,
    {
        self.lookup(key).map(|x| unsafe { &x.as_ref().value })
    }

    /// Returns a mutable reference to the value corresponding to the key.
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        Q: Hash + Ord + ?Sized,
        K: core::borrow::Borrow<Q>,
    {
        self.lookup(key)
            .map(|mut x| unsafe { &mut x.as_mut().value })
    }

    fn lookup<Q>(&self, key: &Q) -> Option<NonNull<RbTreeNode<K, V>>>
    where
        Q: Hash + Ord + ?Sized,
        K: Hash + Ord + core::borrow::Borrow<Q>,
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

            let n = unsafe { ngx_rbtree_data!(node, RbTreeNode<K, V>, node) };

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
