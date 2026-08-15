//! Roc platform host implementation for Roc's symbol-based host ABI.
//!
//! This host provides memory management, I/O effects, and the mltrs renderer
//! for Roc programs.

use std::ffi::c_void;
use std::io::{self, BufRead, Write};
use std::mem::ManuallyDrop;

use mltrs::game::Game;

mod game;
mod generated;

// `roc glue rust_glue` emits edition-2021 code: unsafe operations sit directly
// in unsafe fn bodies, which edition 2024 rejects.
#[allow(unsafe_op_in_unsafe_fn)]
mod roc_platform_abi;

use crate::roc_platform_abi::{
    DefaultAllocators, DefaultHandlers, HostStderrLineResult, HostStderrLineResultPayload,
    HostStderrLineResultTag, HostStdinLineResult, HostStdinLineResultPayload,
    HostStdinLineResultTag, HostStdoutLineResult, HostStdoutLineResultPayload,
    HostStdoutLineResultTag, RocHost, RocStr, make_roc_host, roc_init,
};

static mut ROC_HOST: *mut RocHost = core::ptr::null_mut();

fn set_roc_host(roc_host: *mut RocHost) {
    unsafe {
        ROC_HOST = roc_host;
    }
}

fn roc_host_ptr() -> *mut RocHost {
    unsafe {
        if ROC_HOST.is_null() {
            eprintln!("roc host error: RocHost not initialized");
            std::process::exit(1);
        }
        ROC_HOST
    }
}

fn roc_host() -> &'static RocHost {
    unsafe { &*roc_host_ptr() }
}

fn stderr_line_ok() -> HostStderrLineResult {
    HostStderrLineResult {
        payload: HostStderrLineResultPayload { ok: [] },
        tag: HostStderrLineResultTag::Ok,
    }
}

fn stderr_line_err(err: impl std::fmt::Display) -> HostStderrLineResult {
    HostStderrLineResult {
        payload: HostStderrLineResultPayload {
            err: ManuallyDrop::new(RocStr::from_str(&err.to_string(), roc_host())),
        },
        tag: HostStderrLineResultTag::Err,
    }
}

fn stdin_line_ok(line: RocStr) -> HostStdinLineResult {
    HostStdinLineResult {
        payload: HostStdinLineResultPayload {
            ok: ManuallyDrop::new(line),
        },
        tag: HostStdinLineResultTag::Ok,
    }
}

fn stdin_line_err(err: impl std::fmt::Display) -> HostStdinLineResult {
    HostStdinLineResult {
        payload: HostStdinLineResultPayload {
            err: ManuallyDrop::new(RocStr::from_str(&err.to_string(), roc_host())),
        },
        tag: HostStdinLineResultTag::Err,
    }
}

fn stdout_line_ok() -> HostStdoutLineResult {
    HostStdoutLineResult {
        payload: HostStdoutLineResultPayload { ok: [] },
        tag: HostStdoutLineResultTag::Ok,
    }
}

fn stdout_line_err(err: impl std::fmt::Display) -> HostStdoutLineResult {
    HostStdoutLineResult {
        payload: HostStdoutLineResultPayload {
            err: ManuallyDrop::new(RocStr::from_str(&err.to_string(), roc_host())),
        },
        tag: HostStdoutLineResultTag::Err,
    }
}

/// Hosted function: Host.stderr_line!
#[unsafe(no_mangle)]
pub extern "C" fn roc_stderr_line(message: RocStr) -> HostStderrLineResult {
    let result = writeln!(io::stderr(), "{}", message.as_str());
    // Safety: the hosted function owns `message` and this is its only decref.
    unsafe { message.decref(roc_host()) };

    match result {
        Ok(()) => stderr_line_ok(),
        Err(err) => stderr_line_err(err),
    }
}

/// Hosted function: Host.stdin_line!
#[unsafe(no_mangle)]
pub extern "C" fn roc_stdin_line() -> HostStdinLineResult {
    let stdin = io::stdin();
    let mut line = String::new();

    match stdin.lock().read_line(&mut line) {
        Ok(_) => {
            let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
            stdin_line_ok(RocStr::from_str(trimmed, roc_host()))
        }
        Err(err) => stdin_line_err(err),
    }
}

/// Hosted function: Host.stdout_line!
#[unsafe(no_mangle)]
pub extern "C" fn roc_stdout_line(message: RocStr) -> HostStdoutLineResult {
    let result = writeln!(io::stdout(), "{}", message.as_str());
    // Safety: the hosted function owns `message` and this is its only decref.
    unsafe { message.decref(roc_host()) };

    match result {
        Ok(()) => stdout_line_ok(),
        Err(err) => stdout_line_err(err),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn roc_alloc(length: usize, alignment: usize) -> *mut c_void {
    DefaultAllocators::roc_alloc(roc_host_ptr(), length, alignment)
}

#[unsafe(no_mangle)]
pub extern "C" fn roc_dealloc(ptr: *mut c_void, alignment: usize) {
    DefaultAllocators::roc_dealloc(roc_host_ptr(), ptr, alignment);
}

#[unsafe(no_mangle)]
pub extern "C" fn roc_realloc(
    ptr: *mut c_void,
    new_length: usize,
    alignment: usize,
) -> *mut c_void {
    DefaultAllocators::roc_realloc(roc_host_ptr(), ptr, new_length, alignment)
}

#[unsafe(no_mangle)]
pub extern "C" fn roc_dbg(bytes: *const u8, len: usize) {
    DefaultHandlers::roc_dbg(roc_host_ptr(), bytes, len);
}

#[unsafe(no_mangle)]
pub extern "C" fn roc_expect_failed(bytes: *const u8, len: usize) {
    DefaultHandlers::roc_expect_failed(roc_host_ptr(), bytes, len);
}

#[unsafe(no_mangle)]
pub extern "C" fn roc_crashed(bytes: *const u8, len: usize) {
    DefaultHandlers::roc_crashed(roc_host_ptr(), bytes, len);
}

/// Call the app's `init!` and take ownership of the window title it returns.
fn window_title_from_roc() -> String {
    let config = unsafe { roc_init() };
    let title = config.window_title.as_str().to_string();
    // Safety: `roc_init` transfers the record to the host, and this is the only
    // decref of its one field.
    unsafe { config.window_title.decref(roc_host()) };
    title
}

/// C-compatible main entry point for the Roc program.
/// This is exported so the linker can find it.
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const i8) -> i32 {
    rust_main()
}

/// Main entry point for the Roc program.
pub fn rust_main() -> i32 {
    let mut roc_host = make_roc_host(core::ptr::null_mut());
    set_roc_host(&mut roc_host);

    let title = window_title_from_roc();
    set_roc_host(core::ptr::null_mut());

    if game::set_window_title(title).is_err() {
        eprintln!("roc host error: window title already set");
        return 1;
    }

    match game::BasicTriangle::run() {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err:?}");
            1
        }
    }
}
