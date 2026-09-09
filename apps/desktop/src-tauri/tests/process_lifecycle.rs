use course_workbench_lib::process::{self, ProcessControl};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

#[test]
#[ignore = "invoked only as an owned subprocess by lifecycle tests"]
fn subprocess_helper() {
    if std::env::var("CW_TEST_ROLE").as_deref() == Ok("tree") {
        let mut command =
            process::command(std::env::current_exe().unwrap().to_str().unwrap()).unwrap();
        let mut child = command
            .args(["--ignored", "--exact", "subprocess_helper", "--nocapture"])
            .env("CW_TEST_ROLE", "leaf")
            .spawn()
            .unwrap();
        println!("cw-child:{}", child.id());
        let _ = child.wait();
    }
    std::thread::sleep(Duration::from_secs(60));
}

fn helper(role: &str) -> std::process::Command {
    let mut command = process::command(std::env::current_exe().unwrap().to_str().unwrap()).unwrap();
    command
        .args(["--ignored", "--exact", "subprocess_helper", "--nocapture"])
        .env("CW_TEST_ROLE", role);
    command
}

#[test]
fn nonreading_stdin_is_still_subject_to_the_deadline() {
    let temp = tempfile::tempdir().unwrap();
    let control = ProcessControl::default();
    let started = Instant::now();
    let payload = vec![b'x'; 1024 * 1024];
    let error = process::run_lines(
        helper("leaf"),
        Some(&payload),
        &control,
        &temp.path().join("log"),
        Duration::from_millis(400),
        |_| Ok(()),
    )
    .unwrap_err();
    assert!(error.to_string().contains("超时"));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(control.pid.load(Ordering::SeqCst), 0);
}

#[cfg(windows)]
#[test]
fn cancellation_terminates_the_whole_owned_process_tree() {
    let temp = tempfile::tempdir().unwrap();
    let control = ProcessControl::default();
    let mut child_pid = 0;
    let result = process::run_lines(
        helper("tree"),
        None,
        &control,
        &temp.path().join("log"),
        Duration::from_secs(10),
        |line| {
            if let Some(pid) = line.strip_prefix("cw-child:") {
                child_pid = pid.parse().unwrap();
                control.cancel();
            }
            Ok(())
        },
    );
    assert!(result.unwrap_err().to_string().contains("取消"));
    assert_ne!(child_pid, 0);
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, child_pid);
        if !handle.is_null() {
            let mut code = 259;
            GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            assert_ne!(code, 259, "grandchild survived cancellation");
        }
    }
}
