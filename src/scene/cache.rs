//! A tiny memo table for the scene's sprites and gradients.
//!
//! Building terrain or a sky is far more work than drawing a frame, so results are kept per
//! thread. When a table is full it is simply emptied; the keys in play refill it within a frame.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

/// The last few results of something that rarely changes from one frame to the next (the date and
/// temperature text). Lookups compare keys in place, so a hit allocates nothing.
pub struct Recent<K, V> {
    items: RefCell<Vec<(K, Rc<V>)>>,
    cap: usize,
}

impl<K, V> Recent<K, V> {
    pub const fn new(cap: usize) -> Self {
        Recent { items: RefCell::new(Vec::new()), cap }
    }

    /// The value whose key satisfies `matches`, else `make()`'s (key, value), remembered.
    pub fn get_by(&self, matches: impl Fn(&K) -> bool, make: impl FnOnce() -> (K, V)) -> Rc<V> {
        if let Some((_, v)) = self.items.borrow().iter().find(|(k, _)| matches(k)) {
            return Rc::clone(v);
        }
        let (k, v) = make();
        let v = Rc::new(v);
        let mut items = self.items.borrow_mut();
        if items.len() >= self.cap {
            items.remove(0);
        }
        items.push((k, Rc::clone(&v)));
        v
    }
}

pub struct Memo<K, V> {
    map: RefCell<HashMap<K, Rc<V>>>,
    cap: usize,
}

impl<K: Hash + Eq, V> Memo<K, V> {
    pub fn new(cap: usize) -> Self {
        Memo { map: RefCell::new(HashMap::new()), cap }
    }

    pub fn get(&self, key: K, make: impl FnOnce() -> V) -> Rc<V> {
        if let Some(v) = self.map.borrow().get(&key) {
            return Rc::clone(v);
        }
        let v = Rc::new(make());
        let mut map = self.map.borrow_mut();
        if map.len() >= self.cap {
            map.clear();
        }
        map.insert(key, Rc::clone(&v));
        v
    }
}
