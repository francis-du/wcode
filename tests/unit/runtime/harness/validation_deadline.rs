use super::*;
use std::sync::{mpsc, Barrier};

#[test]
fn native_validation_waiters_release_slots_without_releasing_the_owner() {
    let harness = ToolHarness::new(64).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let permits = runtime.block_on(async {
        let mut permits = Vec::new();
        for _ in 0..64 {
            permits.push(harness.acquire_tool(false).await.unwrap());
        }
        permits
    });
    let mut permits = permits.into_iter();
    let owner_permit = permits.next().unwrap();
    let flight = Arc::new(ValidationFlight::default());
    let owner = flight.gate.lock().unwrap();
    let barrier = Arc::new(Barrier::new(64));
    let (tx, rx) = mpsc::channel();
    let mut workers = Vec::new();
    for permit in permits {
        let flight = flight.clone();
        let barrier = barrier.clone();
        let tx = tx.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            let result = {
                let _participant = ValidationParticipant::join(&flight, 0);
                flight
                    .acquire()
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            };
            drop(permit);
            tx.send(result).unwrap();
        }));
    }
    drop(tx);
    assert_eq!(harness.admission_snapshot().slots_in_use, 64);
    barrier.wait();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut results = Vec::new();
    while results.len() < 63 {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(result) => results.push(result),
            Err(_) => break,
        }
    }
    let waiting_slots = harness.admission_snapshot().slots_in_use;
    let participants = flight.coalescible_callers.load(Ordering::Acquire);
    let owner_preserved = flight.gate.try_lock().is_err();
    let generation = flight.generation.load(Ordering::Acquire);
    // Always let the old implementation unwind before asserting a regression.
    // A failing negative control must not strand the test runner's threads.
    drop(owner);
    drop(owner_permit);
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(
        results.len(),
        63,
        "native waiters retained tool slots indefinitely"
    );
    assert!(results.iter().all(|result| result
        .as_ref()
        .is_err_and(|error| error.contains("validation busy after 5s"))));
    assert_eq!(
        waiting_slots, 1,
        "the live owner's permit must remain occupied"
    );
    assert_eq!(participants, 0);
    assert!(owner_preserved);
    assert_eq!(generation, 0);
    assert!(!flight.can_reuse_after(0));
    assert_eq!(harness.admission_snapshot().slots_in_use, 0);
    let recovered = flight.acquire().unwrap();
    drop(recovered);
    let permit = runtime.block_on(harness.acquire_tool(false)).unwrap();
    drop(permit);
    assert_eq!(harness.admission_snapshot().slots_in_use, 0);
}

#[test]
fn native_validation_waiter_still_coalesces_before_deadline() {
    let flight = Arc::new(ValidationFlight::default());
    let owner = flight.gate.lock().unwrap();
    let worker_flight = flight.clone();
    let (started, ready) = mpsc::channel();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started.send(()).unwrap();
        let result = worker_flight
            .acquire()
            .map(|_| ())
            .map_err(|error| error.to_string());
        tx.send(result).unwrap();
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let early = rx.recv_timeout(Duration::from_millis(50));
    drop(owner);
    let result = early
        .clone()
        .unwrap_or_else(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap());
    worker.join().unwrap();
    assert!(
        early.is_err(),
        "native contention should not discard normal coalescing"
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn native_validation_poison_recovery_does_not_mint_success() {
    let flight = Arc::new(ValidationFlight::default());
    let worker_flight = flight.clone();
    assert!(std::thread::spawn(move || {
        let _owner = worker_flight.gate.lock().unwrap();
        panic!("intentional validation-owner failure");
    })
    .join()
    .is_err());
    let recovered = flight.acquire().unwrap();
    assert_eq!(flight.generation.load(Ordering::Acquire), 0);
    assert!(!flight.can_reuse_after(0));
    drop(recovered);
}
