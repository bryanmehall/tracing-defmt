use std::future::Future;
use std::sync::Mutex;
use std::task::{Context, RawWaker, RawWakerVTable, Waker};
use tracing_defmt::context::{EnterGuard, Instrument, TraceContext, get_active};

// Since `tracing-defmt` stores the OpenTelemetry context in a global static (via critical_section),
// running tests in parallel threads will cause race conditions and flakey tests.
// This mutex ensures these runtime tests execute serially.
static TEST_MUTEX: Mutex<()> = Mutex::new(());

// A dummy waker for polling futures in tests
fn dummy_waker() -> Waker {
    static VTABLE: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(std::ptr::null(), &VTABLE),
        |_| {},
        |_| {},
        |_| {},
    );
    unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

#[test]
fn test_sync_guard() {
    let _lock = TEST_MUTEX.lock().unwrap();

    // Clear any residual context from other tests
    assert_eq!(get_active(), None);

    let ctx = TraceContext::new_root();

    {
        let _guard = EnterGuard::new(ctx);
        assert_eq!(get_active(), Some(ctx));
    }

    assert_eq!(get_active(), None);
}

#[test]
fn test_trace_id_entropy() {
    let _lock = TEST_MUTEX.lock().unwrap();

    // Clear any residual context from other tests
    assert_eq!(get_active(), None);

    let seed: u64 = 0xDEADBEEFCAFEBABE;
    tracing_defmt::context::init(seed);

    let ctx = TraceContext::new_root();

    // The upper 8 bytes of the trace_id should match our seed
    let mut extracted_seed = [0u8; 8];
    extracted_seed.copy_from_slice(&ctx.trace_id[..8]);
    assert_eq!(u64::from_be_bytes(extracted_seed), seed);
}

#[test]
fn test_async_instrumented_polling() {
    let _lock = TEST_MUTEX.lock().unwrap();

    // Clear any residual context from other tests
    assert_eq!(get_active(), None);

    let ctx1 = TraceContext::new_root();
    let ctx2 = TraceContext::new_root();

    let mut future1 = Box::pin(
        async move {
            // When polling future1, context should be ctx1
            assert_eq!(get_active(), Some(ctx1));
            std::future::pending::<()>().await;
        }
        .instrument(ctx1),
    );

    let mut future2 = Box::pin(
        async move {
            // When polling future2, context should be ctx2
            assert_eq!(get_active(), Some(ctx2));
            std::future::pending::<()>().await;
        }
        .instrument(ctx2),
    );

    let waker = dummy_waker();
    let mut cx = Context::from_waker(&waker);

    // Poll future 1
    let _ = future1.as_mut().poll(&mut cx);
    assert_eq!(get_active(), None); // Context should be restored to None after poll

    // Poll future 2
    let _ = future2.as_mut().poll(&mut cx);
    assert_eq!(get_active(), None);
}
