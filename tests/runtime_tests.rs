use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use tracing_defmt::context::{EnterGuard, Instrument, TraceContext, get_active};

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
    assert_eq!(get_active(), None);

    let ctx = TraceContext::new_root();

    {
        let _guard = EnterGuard::new(ctx);
        assert_eq!(get_active(), Some(ctx));
    }

    assert_eq!(get_active(), None);
}

#[test]
fn test_async_instrumented_polling() {
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
