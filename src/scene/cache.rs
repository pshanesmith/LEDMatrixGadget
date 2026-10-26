//! A tiny memo table for the scene's sprites and gradients.
//!
//! Building terrain or a sky is far more work than drawing a frame, so results are kept per
//! thread. When a table is full it is simply emptied; the keys in play refill it within a frame.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

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
