use super::*;

#[test]
fn runtime_activity_counts_separate_verification_and_durable_jobs_from_general_tasks() {
    let monitor = TaskMonitor::new(["project".to_owned()]);

    let verification_running = monitor.queue("project", "verify_project", "verification", 0);
    verification_running.start();
    let _verification_queued = monitor.queue("project", "verify_project", "verification", 0);

    let job_running = monitor.queue("project", "run_command", "durable", 0);
    job_running.bind_command_job("job-running");
    job_running.start();
    let job_queued = monitor.queue("project", "run_command", "durable", 0);
    job_queued.bind_command_job("job-queued");

    let synchronous_command = monitor.queue("project", "run_command", "synchronous", 0);
    synchronous_command.start();
    let unrelated = monitor.queue("project", "risk_status", "read-only", 0);
    unrelated.start();

    let status = monitor.connection_status();
    assert_eq!(status.active_verifications, 1);
    assert_eq!(status.queued_verifications, 1);
    assert_eq!(status.active_jobs, 1);
    assert_eq!(status.queued_jobs, 1);
    assert!(status.active_tasks > status.active_jobs + status.active_verifications);
}
