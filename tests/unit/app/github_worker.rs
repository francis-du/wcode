use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tokio::time::timeout;

#[tokio::test]
async fn inbox_worker_ready_shutdown_starts_no_publication() {
    let mut events = Vec::new();
    let summary = run(
        || async { panic!("shutdown started another publication") },
        std::future::ready(Ok(())),
        |event| {
            events.push(event);
            Ok(())
        },
    )
    .await
    .unwrap();
    assert_eq!(summary.polls, 0);
    assert!(!summary.current_acceptance);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["event"], "worker_started");
    assert_eq!(events[1]["event"], "worker_stopped");
}

#[tokio::test]
async fn inbox_worker_shutdown_finishes_current_attempt_without_starting_another() {
    let (stop, stopped) = oneshot::channel();
    let (started, observed_start) = oneshot::channel();
    let (finish, finished) = oneshot::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let reported = events.clone();
    let mut started = Some(started);
    let mut finished = Some(finished);
    let worker = tokio::spawn(async move {
        run(
            move || {
                called.fetch_add(1, Ordering::SeqCst);
                let started = started.take().expect("second attempt after shutdown");
                let finished = finished.take().unwrap();
                async move {
                    let _ = started.send(());
                    finished.await.unwrap();
                    Ok(None)
                }
            },
            async {
                stopped.await.unwrap();
                Ok(())
            },
            move |event| {
                reported.lock().unwrap().push(event);
                Ok(())
            },
        )
        .await
    });
    timeout(Duration::from_secs(2), observed_start)
        .await
        .unwrap()
        .unwrap();
    stop.send(()).unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!worker.is_finished(), "shutdown dropped the active attempt");
    assert_eq!(events.lock().unwrap().len(), 1);
    finish.send(()).unwrap();
    let summary = timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(summary.polls, 1);
    assert_eq!(summary.publications, 0);
    assert_eq!(
        events.lock().unwrap().last().unwrap()["event"],
        "worker_stopped"
    );
}

#[tokio::test]
async fn inbox_worker_failure_is_sanitized_and_retried_without_a_busy_loop() {
    let (stop, stopped) = oneshot::channel();
    let mut stop = Some(stop);
    let mut starts = Vec::new();
    let mut events = Vec::new();
    let summary = timeout(
        Duration::from_secs(2),
        drive(
            || {
                starts.push(Instant::now());
                let result = if starts.len() == 1 {
                    Err(anyhow!("PRIVATE-PAYLOAD-AND-CREDENTIAL"))
                } else {
                    stop.take().unwrap().send(()).unwrap();
                    Ok(None)
                };
                std::future::ready(result)
            },
            async {
                stopped.await.unwrap();
                Ok(())
            },
            |event| {
                events.push(event);
                Ok(())
            },
            Duration::from_millis(10),
            Duration::from_millis(40),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(starts.len(), 2);
    assert!(starts[1].duration_since(starts[0]) >= Duration::from_millis(40));
    assert_eq!(summary.unavailable_polls, 1);
    assert_eq!(summary.publications, 0);
    assert_eq!(summary.polls, 2);
    assert_eq!(events[1]["event"], "publication_unavailable");
    assert!(!serde_json::to_string(&events).unwrap().contains("PRIVATE-"));
}

#[tokio::test]
async fn inbox_worker_idle_polls_are_paced_and_do_not_flood_output() {
    let (stop, stopped) = oneshot::channel();
    let mut stop = Some(stop);
    let mut starts = Vec::new();
    let mut events = Vec::new();
    let summary = timeout(
        Duration::from_secs(2),
        drive(
            || {
                starts.push(Instant::now());
                if starts.len() == 3 {
                    stop.take().unwrap().send(()).unwrap();
                }
                std::future::ready(Ok(None))
            },
            async {
                stopped.await.unwrap();
                Ok(())
            },
            |event| {
                events.push(event);
                Ok(())
            },
            Duration::from_millis(20),
            Duration::from_millis(40),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(summary.polls, 3);
    assert_eq!(events.len(), 2);
    assert!(starts
        .windows(2)
        .all(|pair| pair[1].duration_since(pair[0]) >= Duration::from_millis(20)));
}

#[tokio::test]
async fn inbox_worker_output_and_signal_failures_are_not_successful_shutdowns() {
    let output = run(
        || async { panic!("unobservable worker started") },
        std::future::pending(),
        |_| Err(anyhow!("output unavailable")),
    )
    .await
    .unwrap_err();
    assert!(output.to_string().contains("output unavailable"));
    let signal = run(
        || async { panic!("shutdown failed but polling continued") },
        std::future::ready(Err(anyhow!("signal unavailable"))),
        |_| Ok(()),
    )
    .await
    .unwrap_err();
    assert!(signal.to_string().contains("signal unavailable"));
}
