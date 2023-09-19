use crate::util::{circular_array::CircularArray, interrupt_guard::InterruptGuarded};
use alloc::{borrow::Cow, string::String};
use core::future::Future;
use hashbrown::HashMap;

#[allow(unused)]
macro_rules! log {
    ($level: expr, $s: expr) => {
        if $crate::logger::LOGGER.get_level(module_path!()) <= $level {
            let log = $crate::logger::Log {
                file: file!(),
                line: line!(),
                level: $level,
                message: $s.into()
            };
            $crate::logger::LOGGER.push_log(log);
        }
    };
    ($level: expr, $s: expr $(, $args: expr)*) => {
        if $crate::logger::LOGGER.get_level(module_path!()) <= $level {
            let log = $crate::logger::Log {
                file: file!(),
                line: line!(),
                level: $level,
                message: alloc::format!($s $(, $args)*).into()
            };
            $crate::logger::LOGGER.push_log(log);
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

pub static LOGGER: Logger = Logger::new();

pub struct Log {
    pub file: &'static str,
    pub line: u32,
    pub level: LogLevel,
    pub message: Cow<'static, str>,
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

pub struct Logger {
    levels: InterruptGuarded<Option<HashMap<String, LogLevel>>>,
    logs: InterruptGuarded<CircularArray<Log, 1024>>,
}

impl Logger {
    const fn new() -> Self {
        Logger {
            levels: InterruptGuarded::new(None),
            logs: InterruptGuarded::new(CircularArray::new()),
        }
    }

    pub fn get_level(&self, module: &str) -> LogLevel {
        *self
            .levels
            .lock()
            .as_ref()
            .expect("Logger not initialized")
            .get(module)
            .unwrap_or(&LogLevel::Info)
    }

    pub fn push_log(&self, log: Log) {
        if self.logs.lock().push_back(log).is_err() {
            panic!("Dropped log");
        }
    }

    pub async fn service<F, Fut>(&self, sleep: F)
    where
        Fut: Future<Output = ()>,
        F: Fn(f32) -> Fut,
    {
        loop {
            // FIXME: Proper signaling of logs ready
            while let Some(v) = self.logs.lock().pop_front() {
                println!("{}", v);
            }

            sleep(0.06).await;
        }
    }
}

pub fn init(log_levels: HashMap<String, LogLevel>) {
    *LOGGER.levels.lock() = Some(log_levels);
}

pub async fn service<F, Fut>(sleep: F)
where
    Fut: Future<Output = ()>,
    F: Fn(f32) -> Fut,
{
    LOGGER.service(sleep).await;
}
