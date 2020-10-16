use pmem::alloc::*;
use pmem::cell::*;
use pmem::clone::PClone;
use pmem::stm::*;
use pmem::vec::Vec;
use pmem::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const BUCKETS_MAX: usize = 16;

type P = BuddyAlloc;

type Bucket<K> = Vec<LogRefCell<(K, usize), P>, P>;

pub struct HashMap<K: NVSafe, V: NVSafe> {
    buckets: Vec<LogRefCell<Bucket<K>, P>, P>,
    values: Vec<LogCell<V, P>, P>,
}

unsafe impl<K: NVSafe, V: NVSafe> Send for HashMap<K, V> {}

impl<K: NVSafe + PartialEq + Hash, V: NVSafe> RootObj<P> for HashMap<K, V> {
    fn init(j: &Journal<P>) -> Self { Self::new(j) }
}

impl<K: NVSafe, V: NVSafe> HashMap<K, V>
where
    K: PartialEq + Hash,
{
    pub fn new(j: &Journal<P>) -> Self {
        let mut buckets = Vec::with_capacity(BUCKETS_MAX, j);
        for _ in 0..BUCKETS_MAX {
            buckets.push(LogRefCell::new(Vec::new(), j), j)
        }
        Self {
            buckets,
            values: Vec::new(),
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

    pub fn put(&mut self, key: K, val: V, j: &Journal<P>) {
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

        self.values.push(LogCell::new(val, j), j);
        bucket.push(LogRefCell::new((key, self.values.len() - 1), j), j);
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

    pub fn update_with<F: FnOnce(&V) -> V>(&mut self, key: &K, j: &Journal<P>, f: F) -> bool
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

    pub fn update_inplace_mut<F>(&self, key: &K, j: &Journal<P>, f: F) -> bool
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

    pub fn update_with_or_insert<F: FnOnce(&V) -> V>(&mut self, key: &K, j: &Journal<P>, f: F)
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

        self.values.push(LogCell::new(f(&V::default()), j), j);
        bucket.push(
            LogRefCell::new((key.pclone(j), self.values.len() - 1), j),
            j,
        );
    }

    pub fn or_insert(&mut self, key: &K, val: V, j: &Journal<P>) -> bool
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

        self.values.push(LogCell::new(val, j), j);
        bucket.push(
            LogRefCell::new((key.pclone(j), self.values.len() - 1), j),
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

    pub fn clear(&mut self, j: &Journal<P>) {
        for i in 0..BUCKETS_MAX {
            *self.buckets[i].borrow_mut(j) = Vec::new();
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
