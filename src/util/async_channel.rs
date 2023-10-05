use crate::util::async_mutex::Mutex;

use alloc::{collections::VecDeque, rc::Rc};
use core::task::Poll;

pub struct Sender<T> {
    inner: Rc<Mutex<VecDeque<T>>>,
}

impl<T> Sender<T> {
    pub async fn send(&self, val: T) {
        self.inner.lock().await.push_back(val);
    }
}

struct ReceiverWaiter<'a, T> {
    inner: &'a Mutex<VecDeque<T>>,
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

        match guard.pop_front() {
            Some(v) => Poll::Ready(v),
            None => Poll::Pending,
        }
    }
}

pub struct Receiver<T> {
    inner: Rc<Mutex<VecDeque<T>>>,
}

impl<T> Receiver<T> {
    pub async fn recv(&self) -> T {
        ReceiverWaiter { inner: &self.inner }.await
    }
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Rc::new(Mutex::new(VecDeque::new()));
    let sender = Sender {
        inner: Rc::clone(&inner),
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

        if let Poll::Ready(_) = recv_poll {
            return Err("async receiver had data when it shouldn't".into());
        }
        Ok(())
    });
}
