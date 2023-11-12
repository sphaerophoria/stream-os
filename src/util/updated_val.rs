use crate::util::async_mutex::Mutex;

use alloc::{sync::Arc, vec::Vec};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

struct Inner<T> {
    val: T,
    generation: u64,
    waker: Vec<Waker>,
}

#[derive(Clone)]
pub struct UpdatedVal<T> {
    inner: Arc<Mutex<Inner<T>>>,
}

impl<T: Copy> UpdatedVal<T> {
    pub fn new(val: T) -> UpdatedVal<T> {
        let inner = Inner {
            val,
            generation: 0,
            waker: Vec::new(),
        };

        let inner = Arc::new(Mutex::new(inner));
        UpdatedVal { inner }
    }

    pub async fn write(&self, val: T) {
        let mut guard = self.inner.lock().await;
        guard.val = val;
        guard.generation += 1;

        let wakers = core::mem::take(&mut guard.waker);
        for waker in wakers {
            waker.wake();
        }
    }

    pub async fn wait(&self) -> T {
        let generation = self.inner.lock().await.generation;
        Waiter {
            inner: &self.inner,
            start_generation: generation,
        }
        .await
    }

    pub async fn read(&self) -> T {
        let guard = self.inner.lock().await;
        guard.val
    }
}

struct Waiter<'a, T> {
    inner: &'a Mutex<Inner<T>>,
    start_generation: u64,
}

impl<T: Copy> Future for Waiter<'_, T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let guard = self.inner.lock();
        let guard = core::pin::pin!(guard);
        let mut guard = match guard.poll(cx) {
            Poll::Ready(v) => v,
            Poll::Pending => {
                return Poll::Pending;
            }
        };

        if self.start_generation != guard.generation {
            Poll::Ready(guard.val)
        } else {
            guard.waker.push(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;

    create_test!(test_read_write, {
        let val = UpdatedVal::new(3);

        test_eq!(val.read().await, 3);
        val.write(4).await;
        test_eq!(val.read().await, 4);

        Ok(())
    });

    //create_test!(test_waiter, {
    //    let val = UpdatedVal::new(3);

    //    let mut finished = false;
    //    let waiter = async {
    //        val.wait().await;
    //        finished = true;
    //    };

    //    let writer = async {
    //        test_eq!(finished, false);
    //        val.write(3).await;
    //        test_eq!(finished, true);
    //        Ok(())
    //    }

    //    crate::future::join(waiter, writer).await;
    //    val.write(4).await;

    //    Ok(())
    //});
}
