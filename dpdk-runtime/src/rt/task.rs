use crate::rt::waker::{MyWaker, waker_from_arc};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

pub struct Task {
    future: Option<Pin<Box<dyn Future<Output = ()> + 'static>>>,
    waker: Waker,
    my_waker: Arc<MyWaker>,
}

impl Task {
    pub fn new<F>(future: F) -> Self
    where
        F: Future<Output = ()> + 'static,
    {
        let my_waker = MyWaker::new();
        let waker = waker_from_arc(my_waker.clone());
        my_waker.schedule();
        Task {
            future: Some(Box::pin(future)),
            waker,
            my_waker,
        }
    }

    pub fn poll(&mut self) -> Poll<()> {
        if let Some(future) = self.future.as_mut() {
            let mut cx = Context::from_waker(&self.waker);
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(()) => {
                    self.future = None;
                    Poll::Ready(())
                }
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Ready(())
        }
    }

    pub fn is_scheduled(&self) -> bool {
        self.my_waker.is_scheduled()
    }

    pub fn clear_scheduled(&self) {
        self.my_waker.set_scheduled(false);
    }

    pub fn is_done(&self) -> bool {
        self.future.is_none()
    }
}
