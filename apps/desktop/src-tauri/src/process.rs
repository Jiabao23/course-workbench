//! Structured child processes, bounded waits, and process-tree lifetime control.
use anyhow::{anyhow, bail, Context, Result};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct ProcessControl {
    pub cancelled: AtomicBool,
    pub pid: AtomicU32,
}
impl ProcessControl {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    pub fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::SeqCst) {
            bail!("任务已取消，已完成的分块保留，可重试恢复");
        }
        Ok(())
    }
}

pub fn command(program: &str) -> Result<Command> {
    if program.trim().is_empty() {
        bail!("所需工具未配置，请前往设置选择可执行文件");
    }
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
        .env("PYTHONUTF8", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUNBUFFERED", "1");
    Ok(command)
}

struct ChildGuard {
    child: Child,
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
}
impl ChildGuard {
    fn new(mut child: Child) -> Result<Self> {
        #[cfg(windows)]
        {
            use std::{
                mem::{size_of, zeroed},
                os::windows::io::AsRawHandle,
                ptr,
            };
            use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
            unsafe {
                let job = CreateJobObjectW(ptr::null(), ptr::null());
                if job.is_null() {
                    let _ = child.kill();
                    return Err(std::io::Error::last_os_error().into());
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) == 0
                    || AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0
                {
                    let error = std::io::Error::last_os_error();
                    let _ = child.kill();
                    let _ = child.wait();
                    CloseHandle(job);
                    return Err(error).context("无法建立子进程生命周期保护");
                }
                Ok(Self { child, job })
            }
        }
        #[cfg(not(windows))]
        Ok(Self { child })
    }
    fn stop(&mut self) {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job, 1);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.stop();
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}

pub fn run_lines(
    mut command: Command,
    input: Option<&[u8]>,
    control: &ProcessControl,
    log_path: &Path,
    timeout: Duration,
    mut on_line: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    control.check()?;
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let log = fs::File::create(log_path)?;
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log));
    let child = command
        .spawn()
        .context("无法启动工具，请检查设置中的路径和运行环境")?;
    let mut guard = ChildGuard::new(child)?;
    control.pid.store(guard.child.id(), Ordering::SeqCst);
    // A misconfigured tool might never read stdin. Keep input writes outside
    // the supervisor so cancellation and the deadline still apply.
    let writer = if let Some(bytes) = input {
        let mut stdin = guard.child.stdin.take().context("无法打开子进程输入")?;
        let bytes = bytes.to_vec();
        Some(thread::spawn(move || stdin.write_all(&bytes)))
    } else {
        None
    };
    let stdout = guard.child.stdout.take().context("无法读取子进程输出")?;
    let (sender, receiver) = mpsc::sync_channel(128);
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let started = Instant::now();
    let result = (|| {
        loop {
            control.check()?;
            if started.elapsed() > timeout {
                bail!("工具运行超时，已保存的分块可在重试时恢复");
            }
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => {
                    let line = line?;
                    if line.len() > 32 * 1024 * 1024 {
                        bail!("工具输出超过大小限制");
                    }
                    if !line.trim().is_empty() {
                        on_line(&line)?;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        loop {
            control.check()?;
            if started.elapsed() > timeout {
                bail!("工具运行超时");
            }
            if let Some(status) = guard.child.try_wait()? {
                if !status.success() {
                    let detail = fs::read_to_string(log_path).unwrap_or_default();
                    let tail: Vec<_> = detail.lines().rev().take(6).collect();
                    return Err(anyhow!(
                        "工具退出（{}）：{}",
                        status.code().unwrap_or(-1),
                        tail.into_iter().rev().collect::<Vec<_>>().join("\n")
                    ));
                }
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
    })();
    if result.is_err() {
        guard.stop();
    }
    drop(receiver);
    let _ = reader.join();
    control.pid.store(0, Ordering::SeqCst);
    if let Some(writer) = writer {
        let written = writer.join().map_err(|_| anyhow!("工具输入线程异常"));
        if result.is_ok() {
            written??;
        }
    }
    result
}

pub fn capture(
    command: Command,
    control: &ProcessControl,
    log: &Path,
    timeout: Duration,
) -> Result<String> {
    let mut output = String::new();
    run_lines(command, None, control, log, timeout, |line| {
        if output.len() + line.len() > 32 * 1024 * 1024 {
            bail!("工具响应过大");
        }
        output.push_str(line);
        output.push('\n');
        Ok(())
    })?;
    Ok(output)
}
