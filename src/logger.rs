use crate::util::{
    async_mutex::Mutex,
    atomic_cell::AtomicCell,
    lock_free_queue::{self, Receiver, Sender},
};
use alloc::string::String;
use core::{
    cell::UnsafeCell,
    ops::Deref,
    task::{Poll, Waker},
};
use hashbrown::HashMap;

#[allow(unused)]
macro_rules! log {
    ($level: expr, $s: expr $(, $args: expr)*) => {
        loop {
            #[allow(unused_unsafe)]
            let logger = unsafe {
                match (*$crate::logger::LOGGER.0.get()).as_mut() {
                    Some(v) => v,
                    None =>  break
                }
            };
            if logger.get_level(module_path!()) <= $level {
                let log = $crate::logger::Log {
                    file: file!(),
                    line: line!(),
                    level: $level,
                    message: alloc::format!($s $(, $args)*),
                };
                logger.push_log(log);
            }

            break;
        }
    };
}

#[allow(unused)]
macro_rules! debug {
    ($s: expr) => {
        log!($crate::logger::LogLevel::Debug, $s)
    };
    ($s: expr $(, $args: expr)*) => {
        log!($crate::logger::LogLevel::Debug, $s $(, $args)*)
    };
}

#[allow(unused)]
macro_rules! info {
    ($s: expr) => {
        log!($crate::logger::LogLevel::Info, $s)
    };
    ($s: expr $(, $args: expr)*) => {
        log!($crate::logger::LogLevel::Info, $s $(, $args)*)
    };
}

#[allow(unused)]
macro_rules! warn {
    ($s: expr) => {
        log!($crate::logger::LogLevel::Warning, $s)
    };
    ($s: expr $(, $args: expr)*) => {
        log!($crate::logger::LogLevel::Warning, $s $(, $args)*)
    };
}

#[allow(unused)]
macro_rules! error {
    ($s: expr) => {
        log!($crate::logger::LogLevel::Error, $s)
    };
    ($s: expr $(, $args: expr)*) => {
        log!($crate::logger::LogLevel::Error, $s $(, $args)*)
    };
}

pub static LOGGER: LoggerHolder = LoggerHolder(UnsafeCell::new(None));
pub struct LoggerHolder(pub UnsafeCell<Option<Logger>>);

impl Deref for LoggerHolder {
    type Target = Logger;

    fn deref(&self) -> &Self::Target {
        unsafe { (*self.0.get()).as_mut().expect("Logger not initialized") }
    }
}
unsafe impl Sync for LoggerHolder {}

pub struct Log {
    pub file: &'static str,
    pub line: u32,
    pub level: LogLevel,
    pub message: String,
}

impl core::fmt::Display for Log {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "[{}] {}:{} {}",
            self.level, self.file, self.line, self.message
        ))?;
        Ok(())
    }
}

#[derive(Copy, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[allow(unused)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl core::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Debug => f.write_str("DEBUG"),
            Self::Info => f.write_str("INFO"),
            Self::Warning => f.write_str("WARNING"),
            Self::Error => f.write_str("ERROR"),
        }?;
        Ok(())
    }
}

struct LogWaiter<'a> {
    log_rx: &'a Mutex<Receiver<Log>>,
    waker: &'a AtomicCell<Waker>,
}

impl core::future::Future for LogWaiter<'_> {
    type Output = Log;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        self.waker.store(cx.waker().clone());

        let guard = core::pin::pin!(self.log_rx.lock()).poll(cx);
        let mut guard = match guard {
            Poll::Ready(v) => v,
            Poll::Pending => {
                return Poll::Pending;
            }
        };

        match guard.pop() {
            Some(v) => Poll::Ready(v),
            None => Poll::Pending,
        }
    }
}

pub struct Logger {
    levels: HashMap<String, LogLevel>,
    log_tx: Sender<Log>,
    log_rx: Mutex<Receiver<Log>>,
    waker: AtomicCell<Waker>,
}

impl Logger {
    fn new(levels: HashMap<String, LogLevel>) -> Self {
        let (log_tx, log_rx) = lock_free_queue::channel(1024);
        let log_rx = Mutex::new(log_rx);
        let waker = AtomicCell::new();
        Logger {
            levels,
            log_tx,
            log_rx,
            waker,
        }
    }

    pub fn get_level(&self, module: &str) -> LogLevel {
        *self.levels.get(module).unwrap_or(&LogLevel::Info)
    }

    pub fn push_log(&self, log: Log) {
        if self.log_tx.push(log).is_err() {
            panic!("Dropped log");
        }

        if let Some(waker) = self.waker.get() {
            waker.wake_by_ref();
        }
    }

    pub async fn service(&self) {
        loop {
            let log = LogWaiter {
                log_rx: &self.log_rx,
                waker: &self.waker,
            }
            .await;
            println!("{}", log);
        }
    }
}

pub fn init(log_levels: HashMap<String, LogLevel>) {
    unsafe {
        (*LOGGER.0.get()) = Some(Logger::new(log_levels));
    }
}

pub async fn service() {
    LOGGER.service().await;
}
