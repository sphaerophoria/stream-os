use crate::util::async_mutex::Mutex;

use alloc::sync::Arc;
use core::task::{Poll, Waker};

struct Inner<T> {
    val: Option<T>,
    is_set: bool,
    waker: Option<Waker>,
}

pub struct Sender<T> {
    inner: Arc<Mutex<Inner<T>>>,
}

impl<T> Sender<T> {
    pub async fn send(self, val: T) {
        let mut inner = self.inner.lock().await;

        assert!(inner.val.is_none());

        inner.val = Some(val);
        inner.is_set = true;

        if let Some(waker) = &inner.waker {
            waker.wake_by_ref();
        }
    }
}

struct ReceiverWaiter<'a, T> {
    inner: &'a Mutex<Inner<T>>,
}

impl<T> core::future::Future for ReceiverWaiter<'_, T> {
    type Output = Result<T, AlreadyReceived>;

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

        if guard.is_set && guard.val.is_none() {
            return Poll::Ready(Err(AlreadyReceived));
        }

        if let Some(v) = core::mem::take(&mut guard.val) {
            return Poll::Ready(Ok(v));
        }

        Poll::Pending
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct AlreadyReceived;

pub struct Receiver<T> {
    inner: Arc<Mutex<Inner<T>>>,
}

impl<T> Receiver<T> {
    pub async fn recv(&self) -> Result<T, AlreadyReceived> {
        ReceiverWaiter { inner: &self.inner }.await
    }
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Inner {
        val: None,
        is_set: false,
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

    create_test!(test_submission, {
        let (tx, rx) = channel();

        let val = crate::future::poll_immediate(rx.recv()).await;
        test_true!(val.is_none());

        tx.send(4).await;

        let val = crate::future::poll_immediate(rx.recv()).await;
        test_eq!(val, Some(Ok::<_, AlreadyReceived>(4)));

        let val = crate::future::poll_immediate(rx.recv()).await;
        test_eq!(val, Some(Err::<i32, _>(AlreadyReceived)));

        Ok(())
    });
}
