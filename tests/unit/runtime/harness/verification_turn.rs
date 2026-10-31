use super::*;

#[path = "verification_turn_native.rs"]
mod native;
use crate::workspace::{cargo_contention_lane, CargoContentionLane};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;
use tokio::sync::{RwLock, Semaphore};

fn lane(program: &str, args: &[&str]) -> Option<CargoContentionLane> {
    cargo_contention_lane(
        program,
        &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
    )
}

async fn poll_once<F: Future>(mut future: Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await
}

#[tokio::test]
async fn verification_phase_writer_waits_before_tool_admission_or_command_deadline() {
    let turn = Arc::new(RwLock::new(()));
    let owner = turn.write().await;
    let slots = Arc::new(Semaphore::new(1));
    let entered = AtomicBool::new(false);
    let mut waiting = Box::pin(run_verification_turn(
        lane("cargo", &["check", "--locked"]),
        &turn,
        async {
            let _permit = slots.acquire().await.unwrap();
            entered.store(true, Ordering::SeqCst);
            // The real check creates its bounded command deadline only after this
            // operation is polled. No process, timer or tool slot is needed to wait.
            17
        },
    ));
    assert!(
        poll_once(waiting.as_mut()).await.is_pending(),
        "a sibling writer entered bounded command admission"
    );
    assert!(!entered.load(Ordering::SeqCst));
    assert_eq!(slots.available_permits(), 1);
    drop(owner);
    assert_eq!(waiting.await, 17);
    assert!(entered.load(Ordering::SeqCst));
    assert_eq!(slots.available_permits(), 1);
}

#[tokio::test]
async fn verification_phase_tests_remain_shared_but_a_queued_writer_keeps_priority() {
    let turn = Arc::new(RwLock::new(()));
    let first = turn.read().await;
    assert_eq!(
        run_verification_turn(lane("cargo", &["test", "--locked"]), &turn, async { 1 }).await,
        1
    );
    let mut writer = Box::pin(run_verification_turn(
        lane("cargo", &["check", "--locked"]),
        &turn,
        async { 2 },
    ));
    assert!(poll_once(writer.as_mut()).await.is_pending());
    let mut late = Box::pin(run_verification_turn(
        lane("cargo", &["test", "--locked"]),
        &turn,
        async { 3 },
    ));
    assert!(
        poll_once(late.as_mut()).await.is_pending(),
        "late readers bypassed a queued writer"
    );
    drop(first);
    assert_eq!(writer.await, 2);
    assert_eq!(late.await, 3);
}

#[tokio::test]
async fn verification_phase_cancelling_a_waiter_releases_readers_without_replaying_work() {
    let turn = Arc::new(RwLock::new(()));
    let first = turn.read().await;
    let entered = AtomicBool::new(false);
    let mut writer = Box::pin(run_verification_turn(
        lane("cargo", &["build", "--locked"]),
        &turn,
        async {
            entered.store(true, Ordering::SeqCst);
        },
    ));
    assert!(poll_once(writer.as_mut()).await.is_pending());
    let mut late = Box::pin(run_verification_turn(
        lane("cargo", &["test", "--locked"]),
        &turn,
        async { 3 },
    ));
    assert!(poll_once(late.as_mut()).await.is_pending());
    drop(writer);
    assert!(matches!(poll_once(late.as_mut()).await, Poll::Ready(3)));
    assert!(
        !entered.load(Ordering::SeqCst),
        "cancelled pending work was launched"
    );
    drop(first);
    assert!(turn.try_write().is_ok());
}

#[tokio::test]
async fn verification_phase_read_only_and_non_cargo_checks_do_not_wait_for_writers() {
    let turn = Arc::new(RwLock::new(()));
    let _owner = turn.write().await;
    for (program, args) in [
        ("cargo", vec!["fmt", "--check"]),
        ("cargo", vec!["help", "test"]),
        ("cargo", vec!["search", "serde"]),
        ("git", vec!["diff", "--check"]),
        ("npm", vec!["test"]),
    ] {
        let mut check = Box::pin(run_verification_turn(lane(program, &args), &turn, async {
            9
        }));
        assert!(
            matches!(poll_once(check.as_mut()).await, Poll::Ready(9)),
            "unrelated check blocked: {program} {args:?}"
        );
    }
}

#[tokio::test]
async fn verification_phase_uses_canonical_maintenance_classification_and_is_run_local() {
    let turn = Arc::new(RwLock::new(()));
    let _reader = turn.read().await;
    for args in [
        vec!["clean"],
        vec!["fmt"],
        vec!["fetch"],
        vec!["update"],
        vec!["custom", "search"],
    ] {
        let mut check = Box::pin(run_verification_turn(lane("cargo", &args), &turn, async {
            1
        }));
        assert!(
            poll_once(check.as_mut()).await.is_pending(),
            "maintenance entered with a reader: {args:?}"
        );
    }
    let independent = Arc::new(RwLock::new(()));
    let mut check = Box::pin(run_verification_turn(
        lane("cargo", &["check"]),
        &independent,
        async { 4 },
    ));
    assert!(
        matches!(poll_once(check.as_mut()).await, Poll::Ready(4)),
        "one planned run blocked a different run"
    );
}

#[tokio::test]
async fn verification_phase_panicking_or_cancelled_owner_does_not_strand_its_turn() {
    let turn = Arc::new(RwLock::new(()));
    let owned = Arc::clone(&turn);
    let panic = tokio::spawn(async move {
        run_verification_turn(lane("cargo", &["check"]), &owned, async {
            panic!("fixture panic");
        })
        .await;
    })
    .await
    .unwrap_err();
    assert!(panic.is_panic());
    assert!(turn.try_write().is_ok());
    let mut waiting = Box::pin(run_verification_turn(
        lane("cargo", &["check"]),
        &turn,
        std::future::pending::<()>(),
    ));
    assert!(poll_once(waiting.as_mut()).await.is_pending());
    assert!(
        turn.try_write().is_err(),
        "running operation did not retain its turn"
    );
    drop(waiting);
    assert!(turn.try_write().is_ok());
}
