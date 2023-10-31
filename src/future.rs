use crate::util::lock_free_queue::{self, Receiver, Sender};

use core::{future::Future, pin::Pin};

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
