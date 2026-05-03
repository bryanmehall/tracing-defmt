use core::cell::Cell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use critical_section::Mutex;
use pin_project_lite::pin_project;

/// The W3C OpenTelemetry Trace Context.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceContext {
    pub trace_id: [u8; 16],
    pub span_id: [u8; 8],
    pub parent_span_id: [u8; 8],
    pub trace_flags: u8,
}

// A zero-allocation global registry. 
// Cell allows mutability inside the Mutex without &mut references.
static ACTIVE_CONTEXT: Mutex<Cell<Option<TraceContext>>> = Mutex::new(Cell::new(None));
static NEXT_TRACE_ID: Mutex<Cell<u64>> = Mutex::new(Cell::new(1));
static NEXT_SPAN_ID: Mutex<Cell<u64>> = Mutex::new(Cell::new(1));
static SEED: Mutex<Cell<u64>> = Mutex::new(Cell::new(0));

/// Initialize the TraceId counter with a hardware-specific random seed.
/// This prevents TraceId collisions across reboots.
pub fn init(seed: u64) {
    critical_section::with(|cs| {
        SEED.borrow(cs).set(seed);
    });
}

impl TraceContext {
    pub fn new_root() -> Self {
        let trace_id_half = critical_section::with(|cs| {
            let cell = NEXT_TRACE_ID.borrow(cs);
            let val = cell.get();
            cell.set(val.wrapping_add(1));
            val
        });

        let seed = critical_section::with(|cs| SEED.borrow(cs).get());

        let mut trace_id = [0u8; 16];
        trace_id[..8].copy_from_slice(&seed.to_be_bytes());
        trace_id[8..].copy_from_slice(&trace_id_half.to_be_bytes());

        let span_id_val = critical_section::with(|cs| {
            let cell = NEXT_SPAN_ID.borrow(cs);
            let val = cell.get();
            cell.set(val.wrapping_add(1));
            val
        });

        Self {
            trace_id,
            span_id: span_id_val.to_be_bytes(),
            parent_span_id: [0; 8],
            trace_flags: 1,
        }
    }

    pub fn child(&self) -> Self {
        let span_id_val = critical_section::with(|cs| {
            let cell = NEXT_SPAN_ID.borrow(cs);
            let val = cell.get();
            cell.set(val.wrapping_add(1));
            val
        });

        Self {
            trace_id: self.trace_id,
            span_id: span_id_val.to_be_bytes(),
            parent_span_id: self.span_id,
            trace_flags: self.trace_flags,
        }
    }
}

#[cfg(feature = "cortex-m")]
fn is_in_isr() -> bool {
    cortex_m::register::ipsr::read() != 0
}

#[cfg(not(feature = "cortex-m"))]
fn is_in_isr() -> bool {
    false
}

/// Gets the currently active TraceContext, if any.
pub fn get_active() -> Option<TraceContext> {
    if is_in_isr() {
        return None;
    }
    critical_section::with(|cs| ACTIVE_CONTEXT.borrow(cs).get())
}
/// Sets the currently active TraceContext, returning the previous one.
pub(crate) fn set_active(ctx: Option<TraceContext>) -> Option<TraceContext> {
    critical_section::with(|cs| {
        let cell = ACTIVE_CONTEXT.borrow(cs);
        let prev = cell.get();
        cell.set(ctx);
        prev
    })
}

pin_project! {
    /// A future that maintains tracing context during `poll`.
    pub struct Instrumented<T> {
        #[pin]
        pub inner: core::mem::ManuallyDrop<T>,
        pub span_context: TraceContext,
    }

    impl<T> PinnedDrop for Instrumented<T> {
        fn drop(this: Pin<&mut Self>) {
            let this = this.project();
            // Enter the context before dropping the inner future
            let _enter = EnterGuard::new(*this.span_context);
            // SAFETY: ManuallyDrop::drop is safe since PinnedDrop is called exactly once.
            unsafe {
                core::mem::ManuallyDrop::drop(this.inner.get_unchecked_mut());
            }
        }
    }
}

impl<T: Future> Future for Instrumented<T> {
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        // 1. Swap IN: Disable interrupts for ~160ns to swap the context.
        let previous = set_active(Some(*this.span_context));

        // 2. POLL: Execute the actual user code.
        // Interrupts are ENABLED here. If an interrupt fires, it won't be blocked.
        let inner = unsafe { this.inner.map_unchecked_mut(|v| &mut **v) };
        let result = inner.poll(cx);

        // 3. Swap OUT: Restore the context to whatever it was before this `.poll()`
        set_active(previous);

        result
    }
}

/// Extension trait allowing futures to be instrumented with a `TraceContext`.
pub trait Instrument: Sized {
    /// Instruments this type with the provided `TraceContext`.
    fn instrument(self, span_context: TraceContext) -> Instrumented<Self> {
        Instrumented {
            inner: core::mem::ManuallyDrop::new(self),
            span_context,
        }
    }
}

impl<T: Future> Instrument for T {}

/// A RAII guard that restores the previous `TraceContext` when dropped.
pub struct EnterGuard {
    previous: Option<TraceContext>,
}

impl EnterGuard {
    /// Enters a new trace context, saving the previous one.
    pub fn new(new_context: TraceContext) -> Self {
        let previous = set_active(Some(new_context));
        Self { previous }
    }
}

impl Drop for EnterGuard {
    fn drop(&mut self) {
        set_active(self.previous);
    }
}
