//! Timings for performance work, written as CSV lines to the file `ARCWISE_TRACE` names. Without
//! it set, every call returns at once.
//!
//! - `frame,<at ms>,<cpu ms>`: a frame, from the start of its render to the end of its paint.
//! - `input,<name>,<first ms>,<settled ms>`: an input, to the first frame drawn after it and to
//!   the first frame with nothing on screen still loading.
//! - `task,<name>,<at ms>,<took ms>`: a piece of work off the UI thread.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// An input that never settles is given up on after this long.
const PATIENCE: Duration = Duration::from_secs(10);

struct Trace {
    out: BufWriter<File>,
    epoch: Instant,
    pending: Option<Pending>,
    flushed: Instant,
}

struct Pending {
    name: String,
    at: Instant,
    first: Option<Duration>,
}

static TRACE: OnceLock<Option<Mutex<Trace>>> = OnceLock::new();

/// Opens the trace file, if one is asked for, and starts timing the launch.
pub fn init() {
    let epoch = Instant::now();
    let trace = TRACE.get_or_init(|| {
        let path = std::env::var_os("ARCWISE_TRACE")?;
        let file = File::create(&path)
            .inspect_err(|e| eprintln!("arcwise: cannot write {}: {e}", path.to_string_lossy()))
            .ok()?;
        Some(Mutex::new(Trace {
            out: BufWriter::new(file),
            epoch,
            pending: None,
            flushed: epoch,
        }))
    });
    if trace.is_some() {
        input(|| "launch".into());
    }
}

fn lock() -> Option<MutexGuard<'static, Trace>> {
    let trace = TRACE.get()?.as_ref()?;
    Some(trace.lock().unwrap_or_else(|e| e.into_inner()))
}

pub fn enabled() -> bool {
    TRACE.get().is_some_and(Option::is_some)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.
}

/// Starts timing an input the user waits on, replacing one not yet settled.
pub fn input(name: impl FnOnce() -> String) {
    let Some(mut trace) = lock() else { return };
    trace.pending = Some(Pending {
        name: name(),
        at: Instant::now(),
        first: None,
    });
}

/// Records a frame that began rendering at `start` and has just painted. `settled` says nothing
/// on screen is still loading.
pub fn frame(start: Instant, settled: bool) {
    let Some(mut trace) = lock() else { return };
    let now = Instant::now();
    let line = format!(
        "frame,{:.3},{:.3}\n",
        ms(start - trace.epoch),
        ms(now - start)
    );
    trace.out.write_all(line.as_bytes()).ok();
    let mut flush = now - trace.flushed > Duration::from_secs(1);
    if let Some(pending) = &mut trace.pending
        && pending.at < start
    {
        let first = *pending.first.get_or_insert(now - pending.at);
        let waited = now - pending.at;
        if settled || waited > PATIENCE {
            let settled = if settled {
                format!("{:.3}", ms(waited))
            } else {
                "never".into()
            };
            let line = format!("input,{},{:.3},{settled}\n", pending.name, ms(first));
            trace.out.write_all(line.as_bytes()).ok();
            trace.pending = None;
            flush = true;
        }
    }
    if flush {
        trace.out.flush().ok();
        trace.flushed = now;
    }
}

/// Records work that started at `start` and has just finished.
pub fn task(name: impl FnOnce() -> String, start: Instant) {
    let Some(mut trace) = lock() else { return };
    let now = Instant::now();
    let line = format!(
        "task,{},{:.3},{:.3}\n",
        name(),
        ms(start - trace.epoch),
        ms(now - start)
    );
    trace.out.write_all(line.as_bytes()).ok();
    trace.out.flush().ok();
}
