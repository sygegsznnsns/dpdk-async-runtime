use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{RawWaker, RawWakerVTable, Waker};

pub struct MyWaker {
    scheduled: AtomicBool,
}

unsafe impl Send for MyWaker {}
unsafe impl Sync for MyWaker {}

impl MyWaker {
    pub fn new() -> Arc<Self> {
        Arc::new(MyWaker {
            scheduled: AtomicBool::new(false),
        })
    }

    pub fn is_scheduled(&self) -> bool {
        self.scheduled.load(Ordering::Acquire)
    }

    pub fn set_scheduled(&self, val: bool) {
        self.scheduled.store(val, Ordering::Release);
    }

    pub fn schedule(&self) {
        self.scheduled.store(true, Ordering::Release);
    }
}

unsafe fn clone(ptr: *const ()) -> RawWaker {
    let arc = Arc::from_raw(ptr as *const MyWaker);
    let cloned = arc.clone();
    std::mem::forget(arc);
    RawWaker::new(Arc::into_raw(cloned) as *const (), &VTABLE)
}

unsafe fn wake(ptr: *const ()) {
    let arc = Arc::from_raw(ptr as *const MyWaker);
    arc.schedule();
}

unsafe fn wake_by_ref(ptr: *const ()) {
    let arc = Arc::from_raw(ptr as *const MyWaker);
    arc.schedule();
    std::mem::forget(arc);
}

unsafe fn drop(ptr: *const ()) {
    let _ = Arc::from_raw(ptr as *const MyWaker);
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

pub fn waker_from_arc(arc: Arc<MyWaker>) -> Waker {
    let raw = RawWaker::new(Arc::into_raw(arc) as *const (), &VTABLE);
    unsafe { Waker::from_raw(raw) }
}
