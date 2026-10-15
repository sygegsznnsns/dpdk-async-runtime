use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use crate::stats::tsc::Tsc;

#[derive(Clone, Copy)]
struct TimerEntry {
    deadline: u64,
    id: u64,
}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline && self.id == other.id
    }
}

impl Eq for TimerEntry {}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other.deadline.cmp(&self.deadline)
    }
}

pub struct Timer {
    tsc: Tsc,
    heap: BinaryHeap<TimerEntry>,
    wakers: Vec<Option<Waker>>,
    next_id: u64,
}

impl Timer {
    pub fn new(tsc: Tsc) -> Self {
        Timer {
            tsc,
            heap: BinaryHeap::new(),
            wakers: Vec::new(),
            next_id: 0,
        }
    }

    pub fn now_cycles(&self) -> u64 {
        self.tsc.now_cycles()
    }

    pub fn cycles_per_sec(&self) -> u64 {
        self.tsc.cycles_per_sec()
    }

    #[inline]
    pub fn cycles_to_ns(&self, cycles: u64) -> u64 {
        self.tsc.cycles_to_ns(cycles)
    }

    #[inline]
    pub fn ns_to_cycles(&self, ns: u64) -> u64 {
        self.tsc.ns_to_cycles(ns)
    }

    pub fn add_timeout(&mut self, cycles_from_now: u64, waker: Waker) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let deadline = self.tsc.now_cycles().wrapping_add(cycles_from_now);
        self.heap.push(TimerEntry { deadline, id });
        if (id as usize) >= self.wakers.len() {
            self.wakers.resize(id as usize + 1, None);
        }
        self.wakers[id as usize] = Some(waker);
        id
    }

    pub fn cancel_timeout(&mut self, id: u64) {
        if (id as usize) < self.wakers.len() {
            self.wakers[id as usize] = None;
        }
    }

    pub fn process_due(&mut self) -> usize {
        let now = self.tsc.now_cycles();
        let mut woken = 0;
        while let Some(entry) = self.heap.peek() {
            if entry.deadline <= now {
                let entry = self.heap.pop().unwrap();
                if (entry.id as usize) < self.wakers.len() {
                    if let Some(waker) = self.wakers[entry.id as usize].take() {
                        waker.wake();
                        woken += 1;
                    }
                }
            } else {
                break;
            }
        }
        woken
    }

    pub fn next_deadline(&self) -> Option<u64> {
        self.heap.peek().map(|e| e.deadline)
    }
}

pub struct Sleep {
    timer_id: u64,
    deadline: u64,
    timer: Rc<RefCell<Timer>>,
    registered: bool,
}

impl Sleep {
    pub fn new(timer: Rc<RefCell<Timer>>, cycles_from_now: u64) -> Self {
        let deadline = timer.borrow().now_cycles().wrapping_add(cycles_from_now);
        Sleep {
            timer_id: u64::MAX,
            deadline,
            timer,
            registered: false,
        }
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let now = { self.timer.borrow().now_cycles() };
        if now >= self.deadline {
            if self.registered {
                self.timer.borrow_mut().cancel_timeout(self.timer_id);
            }
            return Poll::Ready(());
        }
        if !self.registered {
            let cycles = self.deadline.wrapping_sub(now);
            let waker = cx.waker().clone();
            let id = self.timer.borrow_mut().add_timeout(cycles, waker);
            self.timer_id = id;
            self.registered = true;
        }
        Poll::Pending
    }
}
