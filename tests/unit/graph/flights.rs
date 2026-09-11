use super::*;

#[test]
fn independent_file_build_is_not_blocked_by_another_flight() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("first.rs"), "fn first() {}\n").unwrap();
    fs::write(root.path().join("second.rs"), "fn second() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let first = index
        .parse_flight(&FileKey::new(workspace.root(), "first.rs"))
        .unwrap();
    let guard = first.gate.lock().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let result = index.ensure_indexed(&workspace, "second.rs", true);
            sender.send(result.is_ok()).unwrap();
        });
        let completed = receiver.recv_timeout(std::time::Duration::from_secs(3));
        drop(guard);
        assert!(completed.unwrap());
    });
}

#[test]
fn parse_flights_are_bounded_reclaimed_and_workspace_scoped() {
    let index = CodeIndex::new().unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut flights = (0..MAX_PARSE_FLIGHTS)
        .map(|number| {
            index
                .parse_flight(&FileKey::new(root.path(), format!("{number}.rs")))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let first = index
        .parse_flight(&FileKey::new(root.path(), "0.rs"))
        .unwrap();
    assert!(Arc::ptr_eq(&first, &flights[0]));
    assert!(index
        .parse_flight(&FileKey::new(root.path(), "overflow.rs"))
        .is_err());
    flights.pop();
    let other_root = tempfile::tempdir().unwrap();
    let other = index
        .parse_flight(&FileKey::new(other_root.path(), "0.rs"))
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &other));
    drop((flights, first, other));
    let fresh = index
        .parse_flight(&FileKey::new(root.path(), "fresh.rs"))
        .unwrap();
    assert_eq!(index.state.lock().unwrap().parsing.len(), 1);
    drop(fresh);
}

#[test]
fn failed_or_panicking_build_does_not_poison_future_file_queries() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("job.rs");
    fs::write(&path, [0xff]).unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    assert!(index.ensure_indexed(&workspace, "job.rs", false).is_err());
    let flight = index
        .parse_flight(&FileKey::new(workspace.root(), "job.rs"))
        .unwrap();
    let worker_flight = flight.clone();
    assert!(std::thread::spawn(move || {
        let _guard = worker_flight.gate.lock().unwrap();
        panic!("synthetic parse-owner panic");
    })
    .join()
    .is_err());
    fs::write(&path, "fn recovered() {}\n").unwrap();
    let recovered = index.ensure_indexed(&workspace, "job.rs", false).unwrap();
    assert!(recovered
        .record
        .symbols
        .iter()
        .any(|symbol| symbol.name == "recovered"));
}

#[test]
fn invalidation_and_memory_trim_reject_late_parse_publication() {
    for mode in ["file", "prefix", "memory", "external_edit"] {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("nested")).unwrap();
        fs::write(root.path().join("nested/job.rs"), "fn old() {}\n").unwrap();
        let workspace = Workspace::new(root.path(), true, false).unwrap();
        let index = CodeIndex::new().unwrap();
        let key = FileKey::new(workspace.root(), "nested/job.rs");
        let flight = index.parse_flight(&key).unwrap();
        let generation = flight.generation.load(Ordering::Acquire);
        let source = workspace.load_source("nested/job.rs").unwrap();
        let parsed = index
            .parse_source(
                workspace.root(),
                &index.config_for_path("nested/job.rs").unwrap(),
                source,
            )
            .unwrap();
        match mode {
            "file" => index.invalidate(workspace.root(), "nested/job.rs"),
            "prefix" => index.invalidate_prefix(workspace.root(), "nested"),
            "memory" => index.trim_memory(true),
            _ => fs::write(
                root.path().join("nested/job.rs"),
                "fn changed_externally() {}\n",
            )
            .unwrap(),
        }
        assert!(
            index
                .store_parsed_file(&workspace, key, parsed, &flight, generation)
                .is_err(),
            "{mode}"
        );
        assert!(index.state.lock().unwrap().files.is_empty(), "{mode}");
        assert!(
            index
                .ensure_indexed(&workspace, "nested/job.rs", true)
                .is_ok(),
            "{mode}"
        );
    }
}
