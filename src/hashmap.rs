use corundum::default::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const BUCKETS_MAX: usize = 16;

type P = BuddyAlloc;

type Bucket<K> = PVec<PRefCell<(K, usize)>>;

pub struct HashMap<K: PSafe, V: PSafe> {
    buckets: PVec<PRefCell<Bucket<K>>>,
    values: PVec<PCell<V>>,
}

unsafe impl<K: PSafe, V: PSafe> Send for HashMap<K, V> {}

impl<K: PSafe + PartialEq + Hash, V: PSafe> RootObj<P> for HashMap<K, V> {
    fn init(j: &Journal) -> Self { Self::new(j) }
}

impl<K: PSafe, V: PSafe> HashMap<K, V>
where
    K: PartialEq + Hash,
{
    pub fn new(j: &Journal) -> Self {
        let mut buckets = PVec::with_capacity(BUCKETS_MAX, j);
        for _ in 0..BUCKETS_MAX {
            buckets.push(PRefCell::new(PVec::new()), j)
        }
        Self {
            buckets,
            values: PVec::new(),
        }
    }

    pub fn get(&self, key: K) -> Option<V> where V: Copy {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let index = (hasher.finish() as usize) % BUCKETS_MAX;

        for e in &*self.buckets[index].borrow() {
            let e = e.borrow();
            if e.0 == key {
                return Some(self.values[e.1].get());
            }
        }
        None
    }

    pub fn put(&mut self, key: K, val: V, j: &Journal) {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let index = (hasher.finish() as usize) % BUCKETS_MAX;
        let mut bucket = self.buckets[index].borrow_mut(j);

        for e in &*bucket {
            let e = e.borrow();
            if e.0 == key {
                self.values[e.1].set(val, j);
                return;
            }
        }

        self.values.push(PCell::new(val), j);
        bucket.push(PRefCell::new((key, self.values.len() - 1)), j);
    }

    pub fn get_ref(&self, key: K) -> Option<&V> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let index = (hasher.finish() as usize) % BUCKETS_MAX;

        for e in &*self.buckets[index].borrow() {
            let e = e.borrow();
            if e.0 == key {
                return Some(self.values[e.1].get_ref());
            }
        }
        None
    }

    pub fn update_with<F: FnOnce(&V) -> V>(&mut self, key: &K, j: &Journal, f: F) -> bool
    where
        V: Default,
        K: PClone<P>,
    {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let index = (hasher.finish() as usize) % BUCKETS_MAX;
        let bucket = self.buckets[index].borrow_mut(j);

        for e in &*bucket {
            let e = e.borrow();
            if e.0 == *key {
                self.values[e.1].replace(f(self.values[e.1].get_ref()), j);
                return true;
            }
        }
        false
    }


    pub fn update_inplace<F>(&self, key: &K, f: F) -> bool
    where
        F: FnOnce(&V)
    {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let index = (hasher.finish() as usize) % BUCKETS_MAX;
        let bucket = self.buckets[index].borrow();

        for e in &*bucket {
            let e = e.borrow();
            if e.0 == *key {
                self.values[e.1].update_inplace(f);
                return true;
            }
        }
        false
    }

    pub fn update_inplace_mut<F>(&self, key: &K, j: &Journal, f: F) -> bool
    where
        F: FnOnce(&mut V)
    {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let index = (hasher.finish() as usize) % BUCKETS_MAX;
        let bucket = self.buckets[index].borrow_mut(j);

        for e in &*bucket {
            let e = e.borrow();
            if e.0 == *key {
                self.values[e.1].update_inplace_mut(j, f);
                return true;
            }
        }
        false
    }

    pub fn update_with_or_insert<F: FnOnce(&V) -> V>(&mut self, key: &K, j: &Journal, f: F)
    where
        V: Default,
        K: PClone<P>,
    {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let index = (hasher.finish() as usize) % BUCKETS_MAX;
        let mut bucket = self.buckets[index].borrow_mut(j);

        for e in &*bucket {
            let e = e.borrow();
            if e.0 == *key {
                self.values[e.1].set(f(self.values[e.1].get_ref()), j);
                return;
            }
        }

        self.values.push(PCell::new(f(&V::default())), j);
        bucket.push(
            PRefCell::new((key.pclone(j), self.values.len() - 1)),
            j,
        );
    }

    pub fn or_insert(&mut self, key: &K, val: V, j: &Journal) -> bool
    where
        K: PClone<P>,
    {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let index = (hasher.finish() as usize) % BUCKETS_MAX;
        let mut bucket = self.buckets[index].borrow_mut(j);

        for e in &*bucket {
            let e = e.borrow();
            if e.0 == *key {
                return false;
            }
        }

        self.values.push(PCell::new(val), j);
        bucket.push(
            PRefCell::new((key.pclone(j), self.values.len() - 1)),
            j,
        );
        true
    }

    pub fn foreach<F: FnMut(&K, &V) -> ()>(&self, mut f: F) {
        for i in 0..BUCKETS_MAX {
            for e in &*self.buckets[i].borrow() {
                let e = e.borrow();
                f(&e.0, self.values[e.1].get_ref());
            }
        }
    }

    pub fn clear(&mut self, j: &Journal) {
        for i in 0..BUCKETS_MAX {
            *self.buckets[i].borrow_mut(j) = PVec::new();
        }
        self.values.clear();
    }

    pub fn is_empty(&self) -> bool {
        for i in 0..BUCKETS_MAX {
            if !self.buckets[i].borrow().is_empty() {
                return false;
            }
        }
        true
    }
}
