use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use core::{cell::OnceCell, fmt::Write, mem::MaybeUninit};
use hashbrown::HashMap;

use crate::util::spinlock::SpinLock;

#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    let c = c as u8;
    for i in 0..n {
        *s.add(i) = c;
    }
    s
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    for i in 0..n {
        *d.add(i) = *s.add(i);
    }
    d
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    for i in 0..n {
        let a = *s1.add(i) as i32;
        let b = *s2.add(i) as i32;
        if a != b {
            return a - b;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) {
    let mut copy: Vec<MaybeUninit<u8>> = alloc::vec::Vec::with_capacity(n);
    unsafe {
        let copy_ptr = copy.as_mut_ptr();
        for i in 0..n {
            (*copy_ptr.add(i)).write(*src.add(i));
        }

        copy.set_len(n);
    }
    memcpy(dest, copy.as_ptr() as *mut u8, n);
}

#[no_mangle]
pub unsafe extern "C" fn strlen(s: *const i8) -> usize {
    let mut i = 0;
    loop {
        if *s.add(i) == 0 {
            return i;
        }
        i += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn strncpy(d: *mut u8, s: *const u8, n: usize) {
    let copy_len = (strlen(s as *const i8) + 1).min(n);
    memcpy(d, s, copy_len);
}

#[no_mangle]
pub unsafe extern "C" fn strdup(s: *const i8) -> *mut i8 {
    let length = strlen(s) + 1;
    let ret = malloc(length);

    memcpy(ret, s as *const u8, length);

    ret as *mut i8
}

#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut u8 {
    let layout = alloc::alloc::Layout::from_size_align(size, 4).unwrap();

    alloc::alloc::alloc(layout)
}

#[no_mangle]
pub unsafe extern "C" fn calloc(nmemb: usize, size: usize) -> *mut u8 {
    let layout = alloc::alloc::Layout::from_size_align(size * nmemb, 4).unwrap();

    alloc::alloc::alloc_zeroed(layout)
}

#[no_mangle]
pub unsafe extern "C" fn realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    if ptr.is_null() {
        return malloc(size);
    }
    let current_size = crate::allocator::get_size_for_allocation(ptr);
    let layout = alloc::alloc::Layout::from_size_align(current_size, 4).unwrap();

    alloc::alloc::realloc(ptr, layout, size)
}

#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut u8) {
    let layout = alloc::alloc::Layout::from_size_align(0, 4).unwrap();

    alloc::alloc::dealloc(ptr, layout)
}

#[no_mangle]
pub unsafe extern "C" fn putchar(c: i32) -> i32 {
    print!("{}", c as u8 as char);
    c
}

#[no_mangle]
pub unsafe extern "C" fn puts(s: *const i8) {
    println!("{}", core::ffi::CStr::from_ptr(s).to_str().unwrap());
}

#[no_mangle]
pub unsafe extern "C" fn strcmp(s1: *const i8, s2: *const i8) -> i32 {
    let s1_len = strlen(s1);
    let s2_len = strlen(s2);
    let cmp_len = s1_len.min(s2_len);

    let cmp = memcmp(s1 as *const u8, s2 as *const u8, s1_len.min(s2_len));

    if cmp != 0 {
        return cmp;
    }

    if s1_len == s2_len {
        return 0;
    }

    if s1_len > s2_len {
        return *s1.add(cmp_len) as i32;
    }

    return -*s2.add(cmp_len) as i32;
}

#[no_mangle]
pub unsafe extern "C" fn strncmp(s1: *const i8, s2: *const i8, n: usize) -> i32 {
    let s1_len = strlen(s1).min(n);
    let s2_len = strlen(s2).min(n);
    let cmp_len = s1_len.min(s2_len);

    // FIXME: Complete duplication with strcmp
    let cmp = memcmp(s1 as *const u8, s2 as *const u8, s1_len.min(s2_len));

    if cmp != 0 {
        return cmp;
    }

    if s1_len == s2_len {
        return 0;
    }

    if s1_len > s2_len {
        return *s1.add(cmp_len) as i32;
    }

    return -*s2.add(cmp_len) as i32;
}

#[no_mangle]
pub unsafe extern "C" fn strcasecmp(s1: *const i8, s2: *const i8) -> i32 {
    let to_lowercase = |s| {
        let lower_s = core::ffi::CStr::from_ptr(s)
            .to_str()
            .unwrap()
            .to_lowercase();
        alloc::ffi::CString::new(lower_s).unwrap()
    };

    strcmp(to_lowercase(s1).as_ptr(), to_lowercase(s2).as_ptr())
}

#[no_mangle]
pub unsafe extern "C" fn strncasecmp(s1: *const i8, s2: *const i8, n: usize) -> i32 {
    // FIXME: mad duplication
    let to_lowercase = |s| {
        let lower_s = core::ffi::CStr::from_ptr(s).to_bytes().to_ascii_lowercase();
        alloc::ffi::CString::new(lower_s).unwrap()
    };

    strncmp(to_lowercase(s1).as_ptr(), to_lowercase(s2).as_ptr(), n)
}

#[no_mangle]
pub unsafe extern "C" fn toupper(val: i32) -> i32 {
    (val as u8).to_ascii_uppercase() as i32
}

#[no_mangle]
pub unsafe extern "C" fn fwrite(ptr: *const u8, size: u32, nmemb: u32, stream: *mut File) {
    assert_eq!(*stream, File::Stdout);

    let size_bytes = size * nmemb;

    let s = core::slice::from_raw_parts(ptr, size_bytes as usize);
    print!("{}", core::str::from_utf8_unchecked(s));
}

struct PointerWriter {
    buf: *mut u8,
    size: usize,
}

impl core::fmt::Write for PointerWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        unsafe {
            let len = s.len().min(self.size);
            self.buf.copy_from(s.as_ptr(), len);

            self.buf = self.buf.add(len);
            self.size -= len;
        }

        Ok(())
    }
}

pub struct PrintfWriter {
    original_format_string: *const u8,
    format_string: *const u8,
    writer: Box<dyn Write>,
    write_null: bool,
}

impl PrintfWriter {
    unsafe fn advance(&mut self) -> i32 {
        let start = self.format_string;
        let mut ret = 0;
        let pos = self.format_string.offset_from(self.original_format_string);

        loop {
            let char = *self.format_string;

            if let Some(specifier) = parse_format_specifier(self.format_string) {
                ret = match specifier.format.to_size() {
                    Ok(v) => v,
                    Err(_) => {
                        let format_string =
                            core::ffi::CStr::from_ptr(self.original_format_string as *const i8)
                                .to_str()
                                .unwrap();
                        let pos = self.format_string.offset_from(self.original_format_string);

                        panic!("Unhandled to size in: {}, pos {}", format_string, pos);
                    }
                };
                break;
            }

            if char == 0 && self.write_null {
                self.format_string = self.format_string.add(1);
                break;
            } else if char == 0 {
                break;
            } else {
                self.format_string = self.format_string.add(1);
            }
        }

        let size = self
            .format_string
            .offset_from(start)
            .try_into()
            .expect("Exepcted positive size");
        let s = core::slice::from_raw_parts(start, size);
        let s = core::str::from_utf8_unchecked(s);
        write!(self.writer, "{}", s).unwrap();

        ret
    }

    unsafe fn push_arg(&mut self, arg: *const u8) {
        assert_eq!(*self.format_string, b'%');

        let specifier = parse_format_specifier(self.format_string);

        let specifier = match specifier {
            Some(v) => v,
            None => {
                panic!("Not a valid specifier");
            }
        };

        self.format_string = self.format_string.add(specifier.format_specifier_length);

        match specifier.format {
            Format::Char => {
                write!(self.writer, "{}", *(arg as *const u8) as char).unwrap();
            }
            Format::Int => {
                let arg_s = (*(arg as *const i32)).to_string();
                let arg_s = left_pad(arg_s, '0', specifier.precision);
                write!(self.writer, "{}", arg_s).unwrap();
            }
            Format::Hex => {
                let arg_s = alloc::format!("{:x}", *(arg as *const i32));
                let arg_s = left_pad(arg_s, '0', specifier.precision);
                write!(self.writer, "{}", arg_s).unwrap();
            }
            Format::Octal => {
                let arg_s = alloc::format!("{:o}", *(arg as *const i32));
                let arg_s = left_pad(arg_s, '0', specifier.precision);
                write!(self.writer, "{}", arg_s).unwrap();
            }
            Format::String => {
                let c_str_ptr = *(arg as *const *const i8);
                if c_str_ptr.is_null() {
                    write!(self.writer, "(null)").unwrap();
                } else {
                    let c_str = core::ffi::CStr::from_ptr(*(arg as *const *const i8));
                    write!(
                        self.writer,
                        "{}",
                        core::str::from_utf8_unchecked(c_str.to_bytes())
                    )
                    .unwrap();
                }
            }
            Format::Pointer => {
                let arg = *(arg as *const *const i8);
                write!(self.writer, "{:?}", arg).unwrap();
            }
            Format::Unknown(specifier) => {
                panic!("Unknown format specifier {}", specifier);
            }
        }
    }
}

enum Format {
    Char,
    Int,
    String,
    Hex,
    Octal,
    Pointer,
    Unknown(u8),
}

impl Format {
    fn to_size(&self) -> Result<i32, ()> {
        let ret = match self {
            Format::Octal | Format::Char | Format::Hex | Format::Int => {
                core::mem::size_of::<core::ffi::c_int>() as i32
            }
            Format::Pointer | Format::String => {
                core::mem::size_of::<*const core::ffi::c_char>() as i32
            }
            Format::Unknown(d) => return Err(()),
        };

        Ok(ret)
    }
}

struct ParsedFormatSpecifier {
    format_specifier_length: usize,
    leading_char: Option<u8>,
    output_length: Option<u32>,
    precision: Option<u32>,
    format: Format,
}

unsafe fn parse_format_specifier(mut s: *const u8) -> Option<ParsedFormatSpecifier> {
    let start_s = s;
    if *s != b'%' {
        return None;
    }
    s = s.add(1);

    let (s, flag) = parse_flag(s);
    let (s, width) = parse_width(s);
    let (s, precision) = parse_precision(s);
    let (s, format) = parse_format(s);

    Some(ParsedFormatSpecifier {
        format_specifier_length: s
            .offset_from(start_s)
            .try_into()
            .expect("Unexpected negative offset"),
        leading_char: None,
        output_length: None,
        precision,
        format,
    })
}

enum Flag {}

unsafe fn parse_flag(s: *const u8) -> (*const u8, Option<Flag>) {
    (s, None)
}

unsafe fn parse_width(s: *const u8) -> (*const u8, Option<u32>) {
    (s, None)
}

unsafe fn parse_precision(mut s: *const u8) -> (*const u8, Option<u32>) {
    if *s != b'.' {
        return (s, None);
    }

    s = s.add(1);
    let num_start = s;

    while (*s).is_ascii_digit() {
        s = s.add(1);
    }

    let num_s = core::slice::from_raw_parts(
        num_start,
        s.offset_from(num_start)
            .try_into()
            .expect("Invalid negative range"),
    );
    let num: u32 = core::str::from_utf8_unchecked(num_s)
        .parse()
        .expect("Invalid number");

    (s, Some(num))
}

unsafe fn parse_format(s: *const u8) -> (*const u8, Format) {
    let format = match *s {
        b'i' | b'd' => Format::Int,
        b'c' => Format::Char,
        b'x' => Format::Hex,
        b's' => Format::String,
        b'o' => Format::Octal,
        b'p' => Format::Pointer,
        specifier => Format::Unknown(specifier),
    };

    (s.add(1), format)
}

fn left_pad(s: String, val: char, len: Option<u32>) -> String {
    let len = match len {
        Some(v) => v,
        None => {
            return s;
        }
    };

    if len as usize <= s.len() {
        return s;
    }

    let extra_chars = len as usize - s.len();
    alloc::format!("{}{}", str::repeat(&val.to_string(), extra_chars), s)
}

#[no_mangle]
pub unsafe extern "C" fn printf_parser_new(format_string: *const u8) -> *mut PrintfWriter {
    info!("Constrcuted parser");

    let stdout_local = &mut *crate::io::PRINTER.inner.get();
    let stdout_local = match stdout_local {
        Some(v) => v,
        None => return core::ptr::null_mut(),
    };

    let writer = Box::new(&mut **stdout_local);

    Box::leak(Box::new(PrintfWriter {
        original_format_string: format_string,
        format_string,
        writer,
        write_null: false,
    }))
}

#[no_mangle]
pub unsafe extern "C" fn printf_parser_new_with_buf(
    format_string: *const u8,
    buf: *mut u8,
    size: u32,
) -> *mut PrintfWriter {
    let writer = Box::new(PointerWriter {
        buf,
        size: size.try_into().expect("Failed to fit in usize"),
    });

    Box::leak(Box::new(PrintfWriter {
        original_format_string: format_string,
        format_string,
        writer,
        write_null: true,
    }))
}

#[no_mangle]
pub unsafe extern "C" fn printf_parser_new_with_file(
    format_string: *const u8,
    file: *mut File,
) -> *mut PrintfWriter {
    assert_eq!(*file, File::Stdout);
    printf_parser_new(format_string)
}

#[no_mangle]
pub unsafe extern "C" fn printf_parser_advance(parser: *mut PrintfWriter) -> i32 {
    (*parser).advance()
}

#[no_mangle]
pub unsafe extern "C" fn printf_parser_push_arg(parser: *mut PrintfWriter, arg: *const u8) {
    (*parser).push_arg(arg)
}

#[no_mangle]
pub unsafe extern "C" fn printf_parser_free(parser: *mut PrintfWriter) {
    let _ = Box::from_raw(parser);
}

#[no_mangle]
pub unsafe fn print_address(address: *mut u8) {
    println!("address: {:?}", address);
}

#[derive(Debug)]
pub enum File {
    Stdout,
    File(FileCursor<'static>),
}

unsafe impl Sync for File {}

impl core::cmp::PartialEq<File> for File {
    fn eq(&self, other: &File) -> bool {
        match (self, other) {
            (File::Stdout, File::Stdout) => true,
            _ => false,
        }
    }
}

static STDOUT_FILE: File = File::Stdout;

#[repr(transparent)]
struct StaticFile(*const File);
unsafe impl Send for StaticFile {}
unsafe impl Sync for StaticFile {}

#[no_mangle]
static stdout: StaticFile = StaticFile(&STDOUT_FILE);

#[no_mangle]
static stderr: StaticFile = StaticFile(&STDOUT_FILE);

#[derive(Debug)]
struct FileCursor<'a> {
    pos: usize,
    data: &'a SpinLock<Vec<u8>>,
}

type FakePath = Box<[u8]>;
type FakeFile = Box<SpinLock<Vec<u8>>>;

struct FakeFilesystem {
    files: OnceCell<HashMap<FakePath, FakeFile>>,
}

impl FakeFilesystem {
    const fn new() -> FakeFilesystem {
        FakeFilesystem {
            files: OnceCell::new(),
        }
    }

    fn files(&mut self) -> &mut HashMap<FakePath, FakeFile> {
        self.files.get_or_init(|| {
            let mut ret = HashMap::new();
            static FREEDOOM_BYTES: &[u8] =
                include_bytes!("../doomgeneric/doomgeneric/freedoom-0.12.1/freedoom1.wad");
            ret.insert(
                "freedoom1.wad".as_bytes().to_vec().into_boxed_slice(),
                Box::new(SpinLock::new(FREEDOOM_BYTES.to_vec())),
            );
            ret
        });
        self.files.get_mut().expect("files uninitialized")
    }

    fn open(&mut self, path: Box<[u8]>, write: bool) -> Option<File> {
        let files = self.files();
        if write {
            let file = files
                .entry(path)
                .or_insert_with(|| Box::new(SpinLock::new(Vec::new())));

            Some(file_to_cursor(file))
        } else {
            files.get(&path).map(|v| file_to_cursor(v))
        }
    }
}

fn file_to_cursor(file: &SpinLock<Vec<u8>>) -> File {
    // NOTE: This is a complete violation of lifetime rules that only works because we know that
    // FakeFilesystem will always have a static lifetime
    let cursor = FileCursor {
        pos: 0,
        data: unsafe { &*(file as *const SpinLock<_>) },
    };

    File::File(cursor)
}

static FILESYSTEM: SpinLock<FakeFilesystem> = SpinLock::new(FakeFilesystem::new());

#[no_mangle]
pub unsafe extern "C" fn fopen(path: *const i8, mode: *const i8) -> *mut File {
    let mut fs = FILESYSTEM.lock();

    let mode_str = core::ffi::CStr::from_ptr(mode);
    let mode_str = mode_str.to_str().expect("Invalid mode string for fopen");

    let path_s = core::ffi::CStr::from_ptr(path)
        .to_str()
        .expect("Invalid path");

    let path = path_s.as_bytes().to_vec().into_boxed_slice();

    let ret = fs.open(path, mode_str.find('w').is_some());

    match ret {
        Some(f) => Box::leak(Box::new(f)),
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn fclose(f: *mut File) -> i32 {
    let _ = Box::from_raw(f);
    0
}

#[no_mangle]
pub unsafe extern "C" fn ftell(f: *mut File) -> core::ffi::c_long {
    match &*f {
        File::File(cursor) => cursor
            .pos
            .try_into()
            .expect("Could not convert cursor position"),
        File::Stdout => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fseek(
    f: *mut File,
    offset: core::ffi::c_long,
    whence: core::ffi::c_int,
) -> core::ffi::c_int {
    match &mut *f {
        File::File(cursor) => {
            match whence {
                1 => {
                    // SEEK_CUR
                    let pos_i: i64 = cursor
                        .pos
                        .try_into()
                        .expect("Failed to convert position to i64");
                    cursor.pos = (pos_i + (offset as i64))
                        .try_into()
                        .expect("Failed to convert new position to usize");
                    0
                }
                2 => {
                    // SEEK_SET
                    cursor.pos = offset
                        .try_into()
                        .expect("Failed to convert offset to usize");
                    0
                }
                3 => {
                    // SEEK_END
                    let file_size = cursor.data.lock().len();
                    cursor.pos = file_size
                        - TryInto::<usize>::try_into(offset)
                            .expect("Failed to convert offset to usize");
                    0
                }
                _ => {
                    panic!("Invalid whence");
                }
            }
        }
        File::Stdout => -1,
    }
}
#[no_mangle]
pub unsafe extern "C" fn fread(ptr: *mut u8, size: usize, nmemb: usize, f: *mut File) -> usize {
    match &mut *f {
        File::File(cursor) => {
            let size_bytes = size * nmemb;
            let data = cursor.data.lock();

            let end_pos = (cursor.pos + size_bytes).min(data.len());
            let ret = (end_pos - cursor.pos) / size;

            ptr.copy_from(data[cursor.pos..].as_ptr(), end_pos - cursor.pos);

            cursor.pos = end_pos;

            ret
        }
        File::Stdout => 0,
    }
}
