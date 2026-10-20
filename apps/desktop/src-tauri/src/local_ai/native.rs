#[cfg(windows)]
mod platform {
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::time::Duration;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA,
        PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER,
        PDH_HQUERY, PDH_MORE_DATA,
    };
    use windows::Win32::System::SystemInformation::GetTickCount64;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    pub(super) fn idle_elapsed(now: u32, last: u32) -> u64 {
        let elapsed = now.wrapping_sub(last);
        // 32 bit tick taşmasını destekle; gelecekteki/sırasız input verisini boşta sayma.
        if elapsed > i32::MAX as u32 {
            0
        } else {
            u64::from(elapsed) / 1_000
        }
    }

    pub fn idle_seconds() -> u64 {
        let mut input = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            if !GetLastInputInfo(&mut input).as_bool() {
                return 0;
            }
            idle_elapsed(GetTickCount64() as u32, input.dwTime)
        }
    }

    pub(super) fn validated_percent(value: &PDH_FMT_COUNTERVALUE) -> Option<f64> {
        if value.CStatus != PDH_CSTATUS_VALID_DATA && value.CStatus != PDH_CSTATUS_NEW_DATA {
            return None;
        }
        // PDH_FMT_DOUBLE ile okunan union alanı yalnız durum doğrulandıktan sonra kullanılır.
        let percent = unsafe { value.Anonymous.doubleValue };
        (percent.is_finite() && (0.0..=100.0).contains(&percent)).then_some(percent)
    }

    struct PerformanceQuery(PDH_HQUERY);

    impl Drop for PerformanceQuery {
        fn drop(&mut self) {
            unsafe {
                let _ = PdhCloseQuery(self.0);
            }
        }
    }

    /// Yalnız açık çalışma isteğinde çağrılır; örnekleme dışında sayaç veya thread bırakmaz.
    pub fn machine_quiet() -> bool {
        unsafe {
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return false;
            }
            let query = PerformanceQuery(query);
            let mut cpu = PDH_HCOUNTER::default();
            let mut gpu = PDH_HCOUNTER::default();
            // İngilizce adlar yerelleştirilmiş Windows kurulumlarında da PDH tarafından çevrilir.
            if PdhAddEnglishCounterW(
                query.0,
                w!("\\Processor(_Total)\\% Processor Time"),
                0,
                &mut cpu,
            ) != 0
                || PdhAddEnglishCounterW(
                    query.0,
                    w!("\\GPU Engine(*)\\Utilization Percentage"),
                    0,
                    &mut gpu,
                ) != 0
                || PdhCollectQueryData(query.0) != 0
            {
                return false;
            }
            std::thread::sleep(Duration::from_millis(250));
            if PdhCollectQueryData(query.0) != 0 {
                return false;
            }
            let mut cpu_value = PDH_FMT_COUNTERVALUE::default();
            if PdhGetFormattedCounterValue(cpu, PDH_FMT_DOUBLE, None, &mut cpu_value) != 0
                || !validated_percent(&cpu_value).is_some_and(|value| value < 20.0)
            {
                return false;
            }
            let mut bytes = 0u32;
            let mut count = 0u32;
            if PdhGetFormattedCounterArrayW(gpu, PDH_FMT_DOUBLE, &mut bytes, &mut count, None)
                != PDH_MORE_DATA
                || bytes == 0
                || bytes > 16 * 1_024 * 1_024
            {
                return false;
            }
            // Yapı hizalaması korunur; tampon GPU instance adlarını da barındırır.
            let capacity = (bytes as usize).div_ceil(size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>());
            let mut buffer = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); capacity];
            if PdhGetFormattedCounterArrayW(
                gpu,
                PDH_FMT_DOUBLE,
                &mut bytes,
                &mut count,
                Some(buffer.as_mut_ptr()),
            ) != 0
                || count == 0
                || count as usize > buffer.len()
            {
                return false;
            }
            let mut gpu_total = 0.0;
            for item in &buffer[..count as usize] {
                let Some(value) = validated_percent(&item.FmtValue) else {
                    return false;
                };
                // Engine toplamı bilerek ihtiyatlıdır; GPU yükünü olduğundan düşük saymaz.
                gpu_total += value;
                if gpu_total >= 10.0 {
                    return false;
                }
            }
            true
        }
    }

    /// Özel ve adsız job yalnız çağıranın yeni başlattığı PID'yi ve onun çocuklarını kapsar.
    pub struct OwnedJob {
        // OwnedHandle Drop, son job handle'ını kapatınca bağlı süreç ağacını Windows sonlandırır.
        _handle: OwnedHandle,
    }

    impl OwnedJob {
        pub fn attach(pid: u32) -> anyhow::Result<Self> {
            anyhow::ensure!(
                pid != 0 && pid != std::process::id(),
                "Invalid owned runtime PID"
            );
            unsafe {
                let job = OwnedHandle::from_raw_handle(CreateJobObjectW(None, PCWSTR::null())?.0);
                let job_handle = HANDLE(job.as_raw_handle());
                let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job_handle,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )?;
                let process = OwnedHandle::from_raw_handle(
                    OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid)?.0,
                );
                AssignProcessToJobObject(job_handle, HANDLE(process.as_raw_handle()))?;
                Ok(Self { _handle: job })
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn idle_seconds() -> u64 {
        0
    }

    pub fn machine_quiet() -> bool {
        false
    }

    pub struct OwnedJob;

    impl OwnedJob {
        pub fn attach(_pid: u32) -> anyhow::Result<Self> {
            Ok(Self)
        }
    }
}

pub use platform::{idle_seconds, machine_quiet, OwnedJob};

#[cfg(all(test, windows))]
mod tests {
    use super::platform::*;
    use windows::Win32::System::Performance::{
        PDH_CSTATUS_NEW_DATA, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_0,
    };

    #[test]
    fn idle_time_handles_tick_wrap_and_rejects_future_input() {
        assert_eq!(idle_elapsed(4_000, 1_000), 3);
        assert_eq!(idle_elapsed(100, u32::MAX - 1_000), 1);
        assert_eq!(idle_elapsed(1_000, 2_000), 0);
        assert_eq!(idle_elapsed(u32::MAX, 0), 0);
    }

    #[test]
    fn unknown_or_nonfinite_counters_never_allow_work() {
        let value = |status, percent| PDH_FMT_COUNTERVALUE {
            CStatus: status,
            Anonymous: PDH_FMT_COUNTERVALUE_0 {
                doubleValue: percent,
            },
        };
        assert_eq!(validated_percent(&value(0, 0.0)), Some(0.0));
        assert_eq!(
            validated_percent(&value(PDH_CSTATUS_NEW_DATA, 19.0)),
            Some(19.0)
        );
        for percent in [f64::NAN, f64::INFINITY, -1.0, 101.0] {
            assert_eq!(validated_percent(&value(0, percent)), None);
        }
        assert_eq!(validated_percent(&value(5, 0.0)), None);
    }

    #[test]
    fn owned_job_rejects_invalid_pid_without_touching_any_process() {
        assert!(OwnedJob::attach(0).is_err());
        assert!(OwnedJob::attach(std::process::id()).is_err());
    }

    #[test]
    fn owned_job_can_be_kept_in_shared_async_state() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<OwnedJob>();
    }

    #[test]
    fn dropping_owned_job_terminates_its_own_hidden_child() {
        use std::os::windows::process::CommandExt;
        use std::process::{Child, Command, Stdio};
        use std::time::{Duration, Instant};
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;

        // Assert/attach/try_wait hata verse bile yalnız bu testin child sürecini temizle.
        struct TestChild(Child);
        impl Drop for TestChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let powershell = std::path::PathBuf::from(
            std::env::var_os("SystemRoot").expect("Windows system directory is required"),
        )
        .join("System32\\WindowsPowerShell\\v1.0\\powershell.exe");
        let mut child = TestChild(
            Command::new(powershell)
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    "Start-Sleep -Seconds 30",
                ])
                .creation_flags(CREATE_NO_WINDOW.0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("The hidden test helper must start"),
        );
        let job = OwnedJob::attach(child.0.id()).expect("The test child must join its private job");
        assert!(child
            .0
            .try_wait()
            .expect("Read the test child status")
            .is_none());
        drop(job);

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if child
                .0
                .try_wait()
                .expect("Read the test child status")
                .is_some()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "The job did not terminate its test child within two seconds"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
