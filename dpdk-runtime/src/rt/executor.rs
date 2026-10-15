use crate::rt::task::Task;
use crate::rt::reactor::Reactor;
use crate::rt::timer::Timer;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

pub struct Executor {
    ready_queue: VecDeque<Task>,
    reactor: Rc<RefCell<Reactor>>,
    timer: Rc<RefCell<Timer>>,
}

impl Executor {
    pub fn new(reactor: Rc<RefCell<Reactor>>, timer: Rc<RefCell<Timer>>) -> Self {
        Executor {
            ready_queue: VecDeque::new(),
            reactor,
            timer,
        }
    }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: Future<Output = ()> + 'static,
    {
        let task = Task::new(future);
        self.ready_queue.push_back(task);
    }

    pub fn reactor(&self) -> Rc<RefCell<Reactor>> {
        self.reactor.clone()
    }

    pub fn timer(&self) -> Rc<RefCell<Timer>> {
        self.timer.clone()
    }

    pub fn run_ready(&mut self) -> usize {
        let mut processed = 0;
        let mut i = 0;
        while i < self.ready_queue.len() {
            let task = self.ready_queue.get_mut(i).unwrap();
            if task.is_done() {
                self.ready_queue.remove(i);
                continue;
            }
            if !task.is_scheduled() {
                i += 1;
                continue;
            }
            task.clear_scheduled();
            match task.poll() {
                Poll::Ready(()) => {
                    self.ready_queue.remove(i);
                    processed += 1;
                }
                Poll::Pending => {
                    i += 1;
                }
            }
        }
        processed
    }

    pub fn block_on<F, T>(&mut self, mut future: F) -> T
    where
        F: Future<Output = T>,
    {
        let main_task = MainTask::new();
        let waker = main_task.waker();
        let mut cx = Context::from_waker(&waker);

        loop {
            {
                let pinned = unsafe { Pin::new_unchecked(&mut future) };
                match pinned.poll(&mut cx) {
                    Poll::Ready(val) => return val,
                    Poll::Pending => {}
                }
            }

            {
                let mut timer = self.timer.borrow_mut();
                timer.process_due();
            }

            {
                let mut reactor = self.reactor.borrow_mut();
                reactor.poll_once();
            }

            self.run_ready();

            let has_scheduled = self.ready_queue.iter().any(|t| t.is_scheduled());
            if !has_scheduled && !main_task.is_woken() {
                core::hint::spin_loop();
            }
            main_task.clear_woken();
            self.run_ready();
        }
    }
}

struct MainTask {
    woken: Arc<Mutex<bool>>,
    waker: Waker,
}

impl MainTask {
    fn new() -> Self {
        let woken = Arc::new(Mutex::new(false));
        let woken_clone = woken.clone();
        let waker = Waker::from(Arc::new(SimpleWaker { woken: woken_clone }));
        MainTask { woken, waker }
    }

    fn waker(&self) -> Waker {
        self.waker.clone()
    }

    fn is_woken(&self) -> bool {
        *self.woken.lock().unwrap()
    }

    fn clear_woken(&self) {
        *self.woken.lock().unwrap() = false;
    }
}

struct SimpleWaker {
    woken: Arc<Mutex<bool>>,
}

impl std::task::Wake for SimpleWaker {
    fn wake(self: Arc<Self>) {
        *self.woken.lock().unwrap() = true;
    }

    fn wake_by_ref(self: &Arc<Self>) {
        *self.woken.lock().unwrap() = true;
    }
}
