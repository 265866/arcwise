//! Timings for performance work, written as CSV lines to the file `ARCWISE_TRACE` names. Without
//! it set, every call returns at once.
//!
//! - `frame,<at ms>,<cpu ms>`: a frame, from the start of its render to the end of its paint.
//! - `input,<name>,<first ms>,<settled ms>`: an input, to the first frame drawn after it and to
//!   the first frame with nothing on screen still loading.
//! - `task,<name>,<at ms>,<took ms>`: a piece of work off the UI thread.

use std::env;
use std::fs::File;
use std::io::{self, BufWriter};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use arcwise_remote::remote;

/// An input that never settles is given up on after this long.
const PATIENCE: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct Trace {
    out: BufWriter<File>,
    epoch: Instant,
    pending: Option<Pending>,
    flushed: Instant,
    /// Whether a write has failed, after which the trace is left as it is.
    failed: bool,
}

#[derive(Debug)]
struct Pending {
    name: String,
    at: Instant,
    first: Option<Duration>,
}

static TRACE: OnceLock<Option<Mutex<Trace>>> = OnceLock::new();

/// Opens the trace file, if one is asked for, and starts timing the launch.
pub(crate) fn init() {
    let epoch = Instant::now();
    let trace = TRACE.get_or_init(|| {
        let path = env::var_os("ARCWISE_TRACE")?;
        let file = File::create(&path)
            .inspect_err(|e| {
                remote::report(format_args!("cannot write {}: {e}", path.to_string_lossy()));
            })
            .ok()?;
        Some(Mutex::new(Trace {
            out: BufWriter::new(file),
            epoch,
            pending: None,
            flushed: epoch,
            failed: false,
        }))
    });
    if trace.is_some() {
        input(|| "launch".into());
    }
}

/// The trace, while one is being written. A panic while another thread held it leaves it alone.
fn lock() -> Option<MutexGuard<'static, Trace>> {
    let trace = TRACE.get()?.as_ref()?.lock().ok()?;
    (!trace.failed).then_some(trace)
}

/// Whether a trace is being written: one was asked for, and it hasn't stopped on a failed write
/// or a panic while another thread held it. The frame hook is added only while it is, and the
/// keystroke hook, added at launch, returns at once after it stops.
#[must_use]
pub(crate) fn enabled() -> bool {
    lock().is_some()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.
}

impl Trace {
    /// Writes `line`. The first failure is reported and stops the trace, since the lines after a
    /// lost one would mislead.
    fn write(&mut self, line: &str) {
        use std::io::Write as _;

        if !self.failed {
            let written = self.out.write_all(line.as_bytes());
            self.check(written);
        }
    }

    /// Sends the lines written so far to the file, so a trace read while Arcwise runs is current.
    fn flush(&mut self) {
        use std::io::Write as _;

        if !self.failed {
            let flushed = self.out.flush();
            self.check(flushed);
        }
    }

    /// Reports the first failure to write the trace, and stops it.
    fn check(&mut self, written: io::Result<()>) {
        if let Err(e) = written {
            remote::report(format_args!("cannot write the trace: {e}"));
            self.failed = true;
        }
    }
}

/// Starts timing an input the user waits on, replacing one not yet settled.
pub(crate) fn input(name: impl FnOnce() -> String) {
    let Some(mut trace) = lock() else { return };
    trace.pending = Some(Pending {
        name: name(),
        at: Instant::now(),
        first: None,
    });
}

/// Records a frame that began rendering at `start` and has just painted. `settled` says nothing
/// on screen is still loading.
pub(crate) fn frame(start: Instant, settled: bool) {
    let Some(mut trace) = lock() else { return };
    let now = Instant::now();
    let line = format!(
        "frame,{:.3},{:.3}\n",
        ms(start - trace.epoch),
        ms(now - start)
    );
    let mut flush = now - trace.flushed > Duration::from_secs(1);
    let mut input = None;
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
            input = Some(format!(
                "input,{},{:.3},{settled}\n",
                pending.name,
                ms(first)
            ));
            flush = true;
        }
    }
    trace.write(&line);
    if let Some(input) = input {
        trace.pending = None;
        trace.write(&input);
    }
    if flush {
        trace.flush();
        trace.flushed = now;
    }
}

/// Records work that started at `start` and has just finished.
pub(crate) fn task(name: impl FnOnce() -> String, start: Instant) {
    let Some(mut trace) = lock() else { return };
    let now = Instant::now();
    let line = format!(
        "task,{},{:.3},{:.3}\n",
        name(),
        ms(start - trace.epoch),
        ms(now - start)
    );
    trace.write(&line);
    trace.flush();
}
