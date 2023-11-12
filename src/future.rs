use crate::{
    multiprocessing::CpuFnDispatcher,
    util::{
        lock_free_queue::{self, Receiver, Sender},
        spinlock::SpinLock,
    },
};

use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
};

use hashbrown::{HashMap, HashSet};

use alloc::{boxed::Box, sync::Arc, task::Wake, vec::Vec};

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
    future: Pin<Box<dyn Future<Output = ()> + Send + 'a>>,
    waker: Arc<KernelWaker>,
}

#[derive(Clone, Debug, Copy, Hash, Eq, PartialEq)]
pub struct TaskId(u64);

pub struct Executor<'a> {
    cpu_dispatcher: Option<&'a CpuFnDispatcher>,
    id: TaskId,
    tasks: Arc<SpinLock<HashMap<TaskId, Task<'a>>>>,
    to_run: Receiver<TaskId>,
    queue_to_run: Sender<TaskId>,
}

impl<'a> Executor<'a> {
    pub fn new(dispatcher: Option<&'a CpuFnDispatcher>) -> Executor<'a> {
        let (queue_to_run, to_run) = lock_free_queue::channel(1024);
        Executor {
            cpu_dispatcher: dispatcher,
            id: TaskId(0),
            tasks: Arc::new(SpinLock::new(Default::default())),
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

        self.tasks.lock().insert(id, task);
        self.queue_to_run
            .push(id)
            .expect("Failed to queue task on executor");
    }

    pub fn run(mut self) {
        loop {
            if self.tasks.lock().is_empty() {
                return;
            }

            let cpus: Vec<_> = if let Some(cpu_dispatcher) = &self.cpu_dispatcher {
                cpu_dispatcher
                    .cpus()
                    .map(|id| (id, Arc::new(AtomicBool::new(false))))
                    .collect()
            } else {
                Vec::new()
            };

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
                let task = match self.tasks.lock().remove(&task_id) {
                    Some(v) => v,
                    None => {
                        error!("Failed to get task {task_id:?}");
                        continue;
                    }
                };

                // NOTE: We are casting to static lifetime, and guaranteeing that we do not exit
                // this loop until all outgoing tasks have completed. This is dangerous, but the
                // wait at the bottom of this loop for tasks to be completed should prevent any
                // problems
                let mut task = unsafe { core::mem::transmute::<Task<'a>, Task<'static>>(task) };
                let tasks = unsafe {
                    core::mem::transmute::<
                        Arc<SpinLock<HashMap<TaskId, Task<'a>>>>,
                        Arc<SpinLock<HashMap<TaskId, Task<'static>>>>,
                    >(Arc::clone(&self.tasks))
                };

                let poll_fn = move || {
                    let context_waker = Arc::clone(&task.waker).into();
                    let mut context = core::task::Context::from_waker(&context_waker);
                    if task.future.as_mut().poll(&mut context).is_pending() {
                        tasks.lock().insert(task_id, task);
                    }
                };

                let mut poll_fn = Some(poll_fn);

                for (id, currently_executing) in &cpus {
                    if currently_executing.load(Ordering::Relaxed) {
                        continue;
                    }

                    currently_executing.store(true, Ordering::Relaxed);
                    #[allow(clippy::needless_borrow)]
                    let currently_executing = Arc::clone(&currently_executing);
                    let poll_fn = core::mem::take(&mut poll_fn).expect("Poll fn should be valid");
                    self.cpu_dispatcher
                        .as_ref()
                        .expect("Trying to dispatch to cpu when dispatcher does not exist")
                        .execute(*id, move || {
                            poll_fn();
                            currently_executing.store(false, Ordering::Release);
                        })
                        .unwrap();
                    break;
                }

                if let Some(poll_fn) = poll_fn {
                    poll_fn();
                }
            }

            for (_, currently_executing) in &cpus {
                while currently_executing.load(Ordering::Acquire) {}
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

#[allow(unused)]
pub fn poll_immediate<R, F: Future<Output = R>>(f: F) -> impl Future<Output = Option<R>> {
    PollImmediate { f: Some(f) }
}
