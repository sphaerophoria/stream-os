use alloc::string::String;

pub struct TestCase {
    pub name: &'static str,
    pub test: &'static (dyn Fn() -> Result<(), String> + Send + Sync),
}

pub fn test_runner(test_fns: &[&TestCase]) {
    for test_case in test_fns {
        print!("{}... ", test_case.name);
        if let Err(e) = (test_case.test)() {
            println!("{}", e);
        } else {
            println!("[ok]");
        }
    }
}

macro_rules! create_test {
    ($name:ident, $content:block) => {
        paste::paste! {
            #[test_case]
            static $name: TestCase = TestCase {
                name: concat!(file!(), " ", stringify!($name)),
                test: &[<$name _test>],
            };
            fn [<$name _test>]() -> Result<(), alloc::string::String> {
                $content
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
