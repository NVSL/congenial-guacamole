

use serde_json::*;
use pmem::stm::Journal;
use pmem::RootObj;
use std::time::SystemTime;
use pmem::prc::VWeak;
use pmem::cell::LogRefCell;
use pmem::prc::Weak;
use pmem::prc::Prc;
use pmem::vec::Vec as PVec;
use pmem::alloc::*;

pub type P = BuddyAlloc;

pub struct Line {
    ts: SystemTime,
    next: LogRefCell<Option<Prc<Line,P>>,P>,
    prev: Weak<Line,P>,
    color: u32,
    points: PVec<(i32,i32),P>
}

impl Line {
    pub fn as_json(&self) -> Value {
        let mut s = vec![];
        for (x,y) in self.points.as_slice() {
            s.push(json!({ "x": x, "y": y }));
        }
        json!({
            "color": self.color,
            "data": s
        })
    }

    pub fn next(&self) -> VWeak<Line, P> {
        let n = self.next.borrow();
        if let Some(n) = &*n {
            n.volatile()
        } else {
            VWeak::null()
        }
    }

    pub fn timestamp(&self) -> SystemTime {
        self.ts
    }

    pub fn points(&self) -> Vec<(i32, i32)> {
        self.points.as_slice().to_vec()
    }

    pub fn color(&self) -> u32 {
        self.color
    }
}

pub struct History {
    head: LogRefCell<Option<Prc<Line,P>>,P>,
    current: LogRefCell<Weak<Line,P>,P>,
}

impl RootObj<P> for History {
    fn init(j: &Journal<P>) -> Self {
        History {
            head: LogRefCell::new(None, j),
            current: LogRefCell::new(Weak::new(), j)
        }
    }
}

impl History {
    pub fn add(&self, j: &Journal<P>, points: &[(i32,i32)], color: u32) {
        let mut current = self.current.borrow_mut(j);
        if let Some(curr) = current.upgrade(j) {
            let mut next = curr.next.borrow_mut(j);
            let new = Prc::new(Line {
                ts: SystemTime::now(),
                next: LogRefCell::new(None, j),
                prev: Prc::downgrade(&curr, j),
                color,
                points: PVec::from_slice(points, j)
            }, j);
            *current = Prc::downgrade(&new, j);
            *next = Some(new);
        } else {
            let mut head = self.head.borrow_mut(j);
            let new = Prc::new(Line {
                ts: SystemTime::now(),
                next: LogRefCell::new(None, j),
                prev: Weak::new(),
                color,
                points: PVec::from_slice(points, j)
            }, j);
            *current = Prc::downgrade(&new, j);
            *head = Some(new);
        }
    }

    pub fn undo(&self) -> bool {
        P::transaction(|j| {
            let mut current = self.current.borrow_mut(j);
            if let Some(curr) = &current.upgrade(j) {
                *current = if let Some(prev) = &curr.prev.upgrade(j) {
                    Prc::downgrade(prev, j)
                } else {
                    Weak::new()
                };
                true
            } else {
                false
            }
        }).unwrap()
    }

    pub fn redo(&self) -> bool {
        P::transaction(|j| {
            let mut current = self.current.borrow_mut(j);
            if let Some(curr) = &current.upgrade(j) {
                if let Some(next) = &*curr.next.borrow() {
                    *current = Prc::downgrade(next, j);
                    true
                } else {
                    false
                }
            } else if let Some(head) = &*self.head.borrow() {
                *current = Prc::downgrade(head, j);
                true
            } else {
                false
            }
        }).unwrap()
    }

    pub fn clear(&self) -> bool {
        P::transaction(|j| {
            let mut current = self.current.borrow_mut(j);
            let mut head  = self.head.borrow_mut(j);
            let res = head.is_some();
            *head = None;
            *current = Weak::new();
            res
        }).unwrap()
    }

    pub fn head(&self) -> VWeak<Line,P> {
        if let Some(head) = &*self.head.borrow() {
            head.volatile()
        } else {
            VWeak::null()
        }
    }

    pub fn last_timestamp(&self, j: &Journal<P>) -> SystemTime {
        if let Some(last) = self.current.borrow().upgrade(j) {
            last.ts
        } else {
            SystemTime::UNIX_EPOCH
        }
    }
}