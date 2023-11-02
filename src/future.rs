use crate::util::lock_free_queue::{self, Receiver, Sender};

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use hashbrown::{HashMap, HashSet};

use alloc::{boxed::Box, sync::Arc, task::Wake};

struct KernelWaker {
    id: TaskId,
    tx: Sender<TaskId>,
}

impl Wake for KernelWaker {
    fn wake(self: Arc<Self>) {
        self.tx.push(self.id).expect("Failed to wake task");
    }
}

struct Task<'a> {
    future: Pin<Box<dyn Future<Output = ()> + 'a>>,
    waker: Arc<KernelWaker>,
}

#[derive(Clone, Debug, Copy, Hash, Eq, PartialEq)]
pub struct TaskId(u64);

pub struct Executor<'a> {
    id: TaskId,
    tasks: HashMap<TaskId, Task<'a>>,
    to_run: Receiver<TaskId>,
    queue_to_run: Sender<TaskId>,
}

impl<'a> Executor<'a> {
    pub fn new() -> Executor<'a> {
        let (queue_to_run, to_run) = lock_free_queue::channel(1024);
        Executor {
            id: TaskId(0),
            tasks: Default::default(),
            to_run,
            queue_to_run,
        }
    }

    pub fn spawn<F: Future<Output = ()> + 'a + Send>(&mut self, fut: F) {
        let id = self.id;
        self.id.0 += 1;

        let waker = Arc::new(KernelWaker {
            id,
            tx: self.queue_to_run.clone(),
        });

        let task = Task {
            future: Box::pin(fut),
            waker,
        };

        self.tasks.insert(id, task);
        self.queue_to_run
            .push(id)
            .expect("Failed to queue task on executor");
    }

    pub fn run(mut self) {
        loop {
            if self.tasks.is_empty() {
                return;
            }

            let mut to_run = HashSet::new();
            while let Some(v) = self.to_run.pop() {
                to_run.insert(v);
            }

            if to_run.is_empty() {
                unsafe {
                    core::arch::asm!("hlt");
                }
                continue;
            }

            for task_id in to_run {
                let task = match self.tasks.get_mut(&task_id) {
                    Some(v) => v,
                    None => {
                        error!("Failed to get task {task_id:?}");
                        continue;
                    }
                };

                let context_waker = Arc::clone(&task.waker).into();
                let mut context = core::task::Context::from_waker(&context_waker);
                if task.future.as_mut().poll(&mut context).is_ready() {
                    self.tasks.remove(&task_id);
                }
            }
        }
    }
}

pub enum Either<A, B> {
    Left(A),
    Right(B),
}

pub struct Select<F1, F2> {
    f1: Option<F1>,
    f2: Option<F2>,
}

impl<R1, R2, F1, F2> Future for Select<F1, F2>
where
    F1: Future<Output = R1> + Unpin,
    F2: Future<Output = R2> + Unpin,
{
    type Output = Either<(R1, F2), (R2, F1)>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let f1 = Pin::new(self.f1.as_mut().expect("Expected f1 to be valid"));
        if let Poll::Ready(ret) = f1.poll(cx) {
            let f2 = core::mem::take(&mut self.f2);
            return Poll::Ready(Either::Left((ret, f2.expect("Expected f2 to be valid"))));
        }

        let f2 = Pin::new(self.f2.as_mut().expect("Expected f2 to be valid"));
        if let Poll::Ready(ret) = f2.poll(cx) {
            let f1 = core::mem::take(&mut self.f1);
            return Poll::Ready(Either::Right((ret, f1.expect("Expected f1 to be valid"))));
        }

        Poll::Pending
    }
}

pub fn select<R1, R2, F1: Future<Output = R1>, F2: Future<Output = R2>>(
    f1: F1,
    f2: F2,
) -> Select<F1, F2> {
    Select {
        f1: Some(f1),
        f2: Some(f2),
    }
}

pub struct PollFn<F> {
    f: F,
}

impl<R, F> Future for PollFn<F>
where
    F: Fn(&mut Context) -> Poll<R>,
{
    type Output = R;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        (self.f)(cx)
    }
}

pub fn poll_fn<R, F: Fn(&mut core::task::Context) -> Poll<R>>(f: F) -> PollFn<F> {
    PollFn { f }
}

pub struct PollImmediate<F> {
    f: Option<F>,
}

impl<R, F> Future for PollImmediate<F>
where
    F: Future<Output = R>,
{
    type Output = Option<R>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_mut = unsafe { self.get_unchecked_mut() };
        let mut f = self_mut
            .f
            .take()
            .expect("PollImmediate polled after completion");
        let f = core::pin::pin!(f);
        match f.poll(cx) {
            Poll::Ready(v) => Poll::Ready(Some(v)),
            Poll::Pending => Poll::Ready(None),
        }
    }
}

pub fn poll_immediate<R, F: Future<Output = R>>(f: F) -> impl Future<Output = Option<R>> {
    PollImmediate { f: Some(f) }
}
