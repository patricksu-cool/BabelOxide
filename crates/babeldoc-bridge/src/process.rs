//! Windows workers join a kill-on-close job before their first thread runs.

use anyhow::{Context, Result};
use tokio::process::{Child, Command};

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        mem::{size_of, zeroed},
        ptr::null,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            },
            Threading::{
                CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
            },
        },
    };

    pub struct ProcessScope(HANDLE);
    // Job handles can be closed from any thread; this type exclusively owns it.
    unsafe impl Send for ProcessScope {}
    unsafe impl Sync for ProcessScope {}

    impl Drop for ProcessScope {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub fn prepare(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }

    pub fn contain_and_resume(child: &mut Child) -> Result<ProcessScope> {
        unsafe {
            let handle = CreateJobObjectW(null(), null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error()).context("Create worker job");
            }
            let scope = ProcessScope(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(std::io::Error::last_os_error()).context("Configure worker job");
            }
            let process = child
                .raw_handle()
                .context("Worker process handle unavailable")?;
            if AssignProcessToJobObject(handle, process as HANDLE) == 0 {
                return Err(std::io::Error::last_os_error())
                    .context("Assign suspended worker to job");
            }
            let pid = child.id().context("Worker PID unavailable")?;
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error())
                    .context("Inspect suspended worker thread");
            }
            let mut entry: THREADENTRY32 = zeroed();
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            let mut valid = Thread32First(snapshot, &mut entry) != 0;
            let mut resumed = false;
            while valid {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        resumed = ResumeThread(thread) == 1;
                        CloseHandle(thread);
                    }
                    break;
                }
                valid = Thread32Next(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            anyhow::ensure!(resumed, "Cannot resume contained worker");
            Ok(scope)
        }
    }
}

#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
pub struct ProcessScope;
#[cfg(not(windows))]
pub fn prepare(_command: &mut Command) {}
#[cfg(not(windows))]
pub fn contain_and_resume(_child: &mut Child) -> Result<ProcessScope> {
    Ok(ProcessScope)
}
