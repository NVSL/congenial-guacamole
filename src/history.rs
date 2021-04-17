

use serde_json::*;
use std::time::SystemTime;
use corundum::default::*;
use prc::*;

pub type P = BuddyAlloc;

pub struct Line {
    ts: SystemTime,
    next: PRefCell<Option<Prc<Line>>>,
    prev: PWeak<Line>,
    color: u32,
    points: PVec<(i32,i32)>
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

    pub fn next(&self) -> VWeak<Line> {
        let n = self.next.borrow();
        if let Some(n) = &*n {
            Prc::demote(n)
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

#[derive(Root)]
pub struct History {
    head: PRefCell<Option<Prc<Line>>>,
    current: PRefCell<PWeak<Line>>,
}

impl History {
    pub fn add(&self, j: &Journal, points: &[(i32,i32)], color: u32) {
        let mut current = self.current.borrow_mut(j);
        if let Some(curr) = current.upgrade(j) {
            let mut next = curr.next.borrow_mut(j);
            let new = Prc::new(Line {
                ts: SystemTime::now(),
                next: PRefCell::new(None),
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
                next: PRefCell::new(None),
                prev: PWeak::new(),
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
                    PWeak::new()
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
            *current = PWeak::new();
            res
        }).unwrap()
    }

    pub fn head(&self) -> VWeak<Line> {
        if let Some(head) = &*self.head.borrow() {
            Prc::demote(head)
        } else {
            VWeak::null()
        }
    }

    pub fn last_timestamp(&self, j: &Journal) -> SystemTime {
        if let Some(last) = self.current.borrow().upgrade(j) {
            last.ts
        } else {
            SystemTime::UNIX_EPOCH
        }
    }
}