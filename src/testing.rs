use crate::future;

use core::{future::Future, pin::Pin};

use alloc::{boxed::Box, string::String};

pub struct TestCase {
    pub name: &'static str,
    pub test:
        &'static (dyn Send + Sync + Fn() -> Pin<Box<dyn Future<Output = Result<(), String>>>>),
}

pub fn test_runner(test_fns: &[&TestCase]) {
    future::execute_fut(async {
        for test_case in test_fns {
            print!("{}... ", test_case.name);
            if let Err(e) = (test_case.test)().await {
                println!("{}", e);
            } else {
                println!("[ok]");
            }
        }
    });
}

macro_rules! create_test {
    ($name:ident, $content:block) => {
        paste::paste! {
            #[test_case]
            static $name: TestCase = TestCase {
                name: concat!(file!(), " ", stringify!($name)),
                test: &[<$name _test>],
            };
            fn [<$name _test>]() -> core::pin::Pin<alloc::boxed::Box<dyn core::future::Future<Output=Result<(), alloc::string::String>>>> {
                alloc::boxed::Box::pin(async {
                    $content
                })
            }
        }
    };
}

macro_rules! test_eq {
    ($a:expr, $b:expr) => {
        if $a != $b {
            return Err(alloc::format!(
                "{}:{} {:?} != {:?}",
                file!(),
                line!(),
                $a,
                $b
            ));
        }
    };
}

macro_rules! test_ne {
    ($a:expr, $b:expr) => {
        if $a == $b {
            return Err(alloc::format!(
                "{}:{} {:?} == {:?}",
                file!(),
                line!(),
                $a,
                $b
            ));
        }
    };
}

macro_rules! test_ge {
    ($a:expr, $b:expr) => {
        if $a < $b {
            return Err(alloc::format!(
                "{}:{} {:?} < {:?}",
                file!(),
                line!(),
                $a,
                $b
            ));
        }
    };
}

macro_rules! test_true {
    ($a:expr) => {
        if !$a {
            return Err(alloc::format!(
                "{}:{} {:?} is not true",
                file!(),
                line!(),
                $a
            ));
        }
    };
}

macro_rules! test_false {
    ($a:expr) => {
        if $a {
            return Err(alloc::format!(
                "{}:{} {:?} is not false",
                file!(),
                line!(),
                $a
            ));
        }
    };
}

macro_rules! test_ok {
    ($a:expr) => {
        if $a.is_err() {
            return Err(alloc::format!("{}:{} {:?} is not ok", file!(), line!(), $a));
        }
    };
}

macro_rules! test_err {
    ($a:expr) => {
        if $a.is_ok() {
            return Err(alloc::format!(
                "{}:{} {:?} is not err",
                file!(),
                line!(),
                $a
            ));
        }
    };
}
