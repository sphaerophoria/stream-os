use crate::util::async_mutex::Mutex;

use alloc::{collections::VecDeque, sync::Arc};
use core::task::{Poll, Waker};

struct Inner<T> {
    queue: VecDeque<T>,
    waker: Option<Waker>,
}

pub struct Sender<T> {
    inner: Arc<Mutex<Inner<T>>>,
}

impl<T> Sender<T> {
    pub async fn send(&self, val: T) {
        let mut inner = self.inner.lock().await;
        inner.queue.push_back(val);
        if let Some(waker) = &inner.waker {
            waker.wake_by_ref();
        }
    }
}

struct ReceiverWaiter<'a, T> {
    inner: &'a Mutex<Inner<T>>,
}

impl<T> core::future::Future for ReceiverWaiter<'_, T> {
    type Output = T;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let guard = core::pin::pin!(self.inner.lock());
        let mut guard = match guard.poll(cx) {
            Poll::Ready(v) => v,
            Poll::Pending => return Poll::Pending,
        };

        guard.waker = Some(cx.waker().clone());

        match guard.queue.pop_front() {
            Some(v) => Poll::Ready(v),
            None => Poll::Pending,
        }
    }
}

pub struct Receiver<T> {
    inner: Arc<Mutex<Inner<T>>>,
}

impl<T> Receiver<T> {
    pub async fn recv(&self) -> T {
        ReceiverWaiter { inner: &self.inner }.await
    }
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Inner {
        queue: VecDeque::new(),
        waker: None,
    };
    let inner = Arc::new(Mutex::new(inner));
    let sender = Sender {
        inner: Arc::clone(&inner),
    };
    let receiver = Receiver { inner };
    (sender, receiver)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;
    use core::future::Future;

    create_test!(test_async_channel, {
        let (tx, rx) = channel();
        tx.send(1).await;
        test_eq!(rx.recv().await, 1);

        let recv_poll = futures::future::poll_fn(|cx| {
            let val = core::pin::pin!(rx.recv()).poll(cx);
            Poll::Ready(val)
        })
        .await;

        if recv_poll.is_ready() {
            return Err("async receiver had data when it shouldn't".into());
        }
        Ok(())
    });
}
