use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use log::{error, info, warn};
use prop_rs_android::{resetprop::ResetProp, sys_prop};
use serde::{Deserialize, Serialize};
use zygisk_api::api::{V4, ZygiskApi};

use crate::config::{DPI_MAX, DPI_MIN};

// ── Companion 侧激活会话跟踪 ─────────────────────────────────────────────────
//
// companion 进程持续运行，static 状态可靠（不受 Zygisk 模块 DlClose 影响）。
// 每个 Apply 请求会先恢复上一个会话的备份，确保多应用并发时不会互相污染。

static ACTIVE_SESSION: Mutex<Option<ActiveSession>> = Mutex::new(None);

struct ActiveSession {
    package: String,
    pid: u32,
    backups: HashMap<String, String>,
    density: Option<u32>,
    original_density: Option<u32>,
    watcher_pid: i32,
}

/// Operation-specific part of a process-scoped session watcher.
///
/// The process lifecycle (foreground/background transitions and process exit)
/// is shared by resetprop and DPI. Only applying/restoring the session state
/// differs, so keep that difference behind this enum instead of duplicating
/// the watcher event loop.
#[derive(Clone)]
struct WatcherAction {
    props: HashMap<String, String>,
    delete_props: Vec<String>,
    backups: Vec<PropBackup>,
    density: Option<u32>,
    original_density: Option<u32>,
}

impl WatcherAction {
    fn spawn_label(&self) -> &'static str {
        if self.density.is_some() {
            "DPI restore watcher"
        } else {
            "restore watcher"
        }
    }

    fn label(&self) -> &'static str {
        if self.density.is_some() {
            "DPI watcher"
        } else {
            "restore watcher"
        }
    }

    fn apply(&self) -> anyhow::Result<()> {
        if !self.props.is_empty() || !self.delete_props.is_empty() {
            apply_props_batch(&self.props, &self.delete_props)?;
        }
        if let Some(density) = self.density {
            set_density(Some(density))?;
        }
        Ok(())
    }

    fn restore(&self) -> anyhow::Result<()> {
        if self.density.is_some() {
            restore_density(self.original_density)?;
        }
        if self.backups.is_empty() {
            Ok(())
        } else {
            restore_props_batch(&self.backups)
        }
    }
}

/// 收割已退出的 watcher 子进程，避免僵尸进程积累。
fn reap_zombie_watchers() {
    loop {
        match unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) } {
            0 | -1 => break,
            _ => {} // 收割到一个僵尸，继续尝试
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CpuSpoofRequest {
    pub pid: u32,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CpuSpoofUnmountRequest {
    pub pid: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WriteLogRequest {
    pub lines: Vec<String>,
}

pub fn spoof_system_props_via_companion(
    api: &mut ZygiskApi<V4>,
    prop_map: &HashMap<String, String>,
    delete_props: &[String],
    package_name: &str,
    density: Option<u32>,
) -> anyhow::Result<()> {
    if prop_map.is_empty() && delete_props.is_empty() && density.is_none() {
        return Ok(());
    }

    let request = CompanionRequest::Apply(ResetpropSessionRequest {
        pid: std::process::id(),
        props: prop_map.clone(),
        delete_props: delete_props.to_vec(),
        package_name: package_name.to_string(),
        density,
    });

    let response = send_companion_command(api, &request)?;
    if response.status != 0 {
        anyhow::bail!(
            response
                .message
                .unwrap_or_else(|| "companion resetprop failed".to_string())
        );
    }

    // companion 侧现在自己管理会话状态和恢复逻辑；
    // Zygisk 模块侧不再需要 ACTIVE_RESET_SESSION。

    Ok(())
}

pub fn send_companion_command(
    api: &mut ZygiskApi<V4>,
    request: &CompanionRequest,
) -> anyhow::Result<CompanionResponse> {
    let payload = serde_json::to_vec(request)?;
    let response = api
        .with_companion(|stream| -> anyhow::Result<CompanionResponse> {
            stream.write_all(&(payload.len() as u32).to_le_bytes())?;
            stream.write_all(&payload)?;
            stream.flush()?;

            let mut len_buf = [0u8; 4];
            stream.read_exact(&mut len_buf)?;
            let resp_len = u32::from_le_bytes(len_buf) as usize;
            let mut resp_buf = vec![0u8; resp_len];
            stream.read_exact(&mut resp_buf)?;

            let resp = serde_json::from_slice::<CompanionResponse>(&resp_buf)?;
            Ok(resp)
        })
        .map_err(|e| anyhow::anyhow!("Failed to talk to companion: {e}"))??;

    Ok(response)
}

pub fn handle_companion_request(stream: &mut UnixStream) {
    // companion 进程不会调用 ZygiskModule::on_load，因此需要自行初始化日志。
    #[cfg(target_os = "android")]
    crate::file_logger::init();

    let request = match read_companion_request(stream) {
        Ok(request) => request,
        Err(err) => {
            error!("Companion failed to parse request: {err}");
            let response = CompanionResponse::err("invalid request");
            if let Err(e) = write_companion_response(stream, &response) {
                warn!("Failed to write companion response: {e}");
            }
            return;
        }
    };

    match request {
        CompanionRequest::Apply(request) => {
            let response = match apply_resetprop_session(request) {
                Ok(backups) => CompanionResponse::ok_with_backups(backups),
                Err(err) => {
                    error!("Companion failed to apply resetprop session: {err}");
                    CompanionResponse::err(err.to_string())
                }
            };
            if let Err(e) = write_companion_response(stream, &response) {
                warn!("Failed to write companion response: {e}");
            }
        }
        CompanionRequest::Restore(request) => {
            let response = match restore_properties(request) {
                Ok(_) => CompanionResponse::ok(),
                Err(err) => {
                    error!("Companion failed to restore properties: {err}");
                    CompanionResponse::err(err.to_string())
                }
            };
            if let Err(e) = write_companion_response(stream, &response) {
                warn!("Failed to write companion response: {e}");
            }
        }
        CompanionRequest::CpuSpoof(request) => {
            crate::cpu_spoof::handle_companion_cpu_spoof(stream, request);
        }
        CompanionRequest::CpuSpoofUnmount(request) => {
            crate::cpu_spoof::handle_companion_cpu_unmount(stream, request);
        }
        CompanionRequest::WriteLog(request) => {
            let response = match write_log_lines(request) {
                Ok(_) => CompanionResponse::ok(),
                Err(err) => {
                    error!("Companion failed to write log: {err}");
                    CompanionResponse::err(err.to_string())
                }
            };
            if let Err(e) = write_companion_response(stream, &response) {
                warn!("Failed to write companion response: {e}");
            }
        }
    }
}

fn read_companion_request(stream: &mut UnixStream) -> anyhow::Result<CompanionRequest> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let payload_len = u32::from_le_bytes(len_buf) as usize;
    if payload_len == 0 {
        anyhow::bail!("empty request payload");
    }

    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload)?;
    let request = serde_json::from_slice::<CompanionRequest>(&payload)?;
    Ok(request)
}

pub(crate) fn write_companion_response(
    stream: &mut UnixStream,
    response: &CompanionResponse,
) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(response)?;
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

/// Rebuild property areas for ALL distinct contexts touched by the given keys.
/// More complete than single-context rebuild; handles custom_props spanning
/// multiple SELinux contexts (e.g. ro.* + debug.* + gsm.*).
fn rebuild_all_contexts(keys_iter: impl Iterator<Item = impl AsRef<str>>) {
    let mut contexts: std::collections::HashSet<String> = std::collections::HashSet::new();
    for key in keys_iter {
        if let Ok(ctx) = sys_prop::get_context(key.as_ref()) {
            contexts.insert(ctx);
        }
    }
    for ctx in &contexts {
        if let Err(e) = sys_prop::rebuild(ctx) {
            warn!("prop area rebuild for {ctx} failed (non-fatal): {e}");
        }
    }
}

fn apply_resetprop_session(
    request: ResetpropSessionRequest,
) -> anyhow::Result<HashMap<String, String>> {
    if request.props.is_empty() && request.delete_props.is_empty() && request.density.is_none() {
        return Ok(HashMap::new());
    }

    if let Some(density) = request.density
        && !(DPI_MIN..=DPI_MAX).contains(&density)
    {
        anyhow::bail!(
            "DPI {} is outside the supported range {}..={}",
            density,
            DPI_MIN,
            DPI_MAX
        );
    }

    // ① 收割已退出的 watcher 僵尸进程
    reap_zombie_watchers();

    // ② 检查是否为同一 package 的重复请求（如多进程 app 的子进程）
    //    同一 package 且旧进程仍存活时跳过恢复 + 重新应用。
    //    如果旧进程已退出，清除旧会话并重新应用（属性可能已被恢复）。
    {
        let guard = ACTIVE_SESSION.lock().unwrap();
        if let Some(ref active) = *guard
            && active.package == request.package_name
        {
            // 检查旧进程是否仍存活
            let old_alive = unsafe { libc::kill(active.pid as i32, 0) } == 0;
            if old_alive && active.density == request.density {
                info!(
                    "Skipping duplicate Apply for package '{}' (pid {}), session already active (old pid {} alive)",
                    request.package_name, request.pid, active.pid
                );
                return Ok(active.backups.clone());
            } else {
                info!(
                    "Old session for package '{}' (pid {}) is dead, clearing and re-applying for new pid {}",
                    request.package_name, active.pid, request.pid
                );
            }
        }
    }

    // ③ 如果存在旧会话，先停止 watcher 并恢复属性/DPI 快照。
    if let Some(old) = ACTIVE_SESSION.lock().unwrap().take() {
        let old_alive = unsafe { libc::kill(old.pid as i32, 0) == 0 };
        if old_alive {
            stop_watcher(old.watcher_pid);
            info!(
                "Restoring previous session (package: {}, {} keys, dpi={:?}) before applying new session for '{}'",
                old.package,
                old.backups.len(),
                old.density,
                request.package_name
            );
            restore_active_session(&old);
        } else {
            wait_for_watcher(old.watcher_pid);
            info!(
                "Previous session '{}' (pid {}) is dead; watcher owns cleanup before applying new session",
                old.package, old.pid
            );
        }
    }

    // ④ 备份当前属性/DPI（旧会话已恢复，此时为真实值）
    let mut backups = Vec::with_capacity(request.props.len() + request.delete_props.len());

    for key in request.props.keys() {
        let original = backup_property(key)?;
        backups.push(PropBackup {
            key: key.clone(),
            original_value: original,
        });
    }

    for key in &request.delete_props {
        let original = backup_property(key)?;
        backups.push(PropBackup {
            key: key.clone(),
            original_value: original,
        });
    }

    let backups_for_response: HashMap<String, String> = backups
        .iter()
        .map(|entry| (entry.key.clone(), entry.original_value.clone()))
        .collect();

    // ⑤ 应用新伪装值并启动统一 watcher。
    let original_density = if request.density.is_some() {
        query_density_override()?
    } else {
        None
    };
    let action = WatcherAction {
        props: request.props.clone(),
        delete_props: request.delete_props.clone(),
        backups: backups.clone(),
        density: request.density,
        original_density,
    };
    if let Err(err) = action.apply() {
        let _ = action.restore();
        return Err(err);
    }

    // ⑥ Fork 恢复 watcher
    let watcher_pid = match spawn_process_state_watcher(request.pid, action.clone()) {
        Ok(pid) => pid,
        Err(e) => {
            error!("Failed to spawn restore watcher: {e}, rolling back applied session");
            let _ = action.restore();
            anyhow::bail!("failed to spawn restore watcher: {e}");
        }
    };

    // ⑦ 存储新会话
    *ACTIVE_SESSION.lock().unwrap() = Some(ActiveSession {
        package: request.package_name.clone(),
        pid: request.pid,
        backups: backups
            .iter()
            .map(|b| (b.key.clone(), b.original_value.clone()))
            .collect(),
        density: request.density,
        original_density,
        watcher_pid,
    });

    Ok(backups_for_response)
}

fn restore_active_session(active: &ActiveSession) {
    if active.density.is_some()
        && let Err(e) = restore_density(active.original_density)
    {
        warn!("Failed to restore old session density: {e}");
    }

    for (key, value) in &active.backups {
        if let Err(e) = apply_resetprop(key, value) {
            warn!("Failed to restore old session key '{key}': {e}");
        }
    }
    if !active.backups.is_empty() {
        rebuild_all_contexts(active.backups.keys());
    }
}

fn run_wm_density(args: &[&str]) -> anyhow::Result<String> {
    let mut command = std::process::Command::new("/system/bin/wm");
    command.arg("density").args(args);

    let output =
        match command.output() {
            Ok(output) => output,
            Err(first_error) => {
                // Some root environments expose Android tools through PATH only.
                let mut fallback = std::process::Command::new("wm");
                fallback.arg("density").args(args).output().map_err(|second_error| {
                anyhow::anyhow!(
                    "failed to execute /system/bin/wm ({first_error}) or wm ({second_error})"
                )
            })?
            }
        };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        anyhow::bail!(
            "wm density {:?} failed with status {}: {}",
            args,
            output.status,
            stderr.trim()
        );
    }

    let mut text = stdout.into_owned();
    if !stderr.trim().is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    Ok(text)
}

fn query_density_override() -> anyhow::Result<Option<u32>> {
    let output = run_wm_density(&[])?;
    for line in output.lines() {
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        if !label.trim().eq_ignore_ascii_case("override density") {
            continue;
        }

        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("reset") || value == "0" {
            return Ok(None);
        }

        let density = value
            .parse::<u32>()
            .map_err(|e| anyhow::anyhow!("invalid Override density value '{value}': {e}"))?;
        return Ok(Some(density));
    }

    // AOSP omits the Override density line when no override is active.
    Ok(None)
}

fn set_density(density: Option<u32>) -> anyhow::Result<()> {
    let density_string;
    let args = if let Some(density) = density {
        density_string = density.to_string();
        vec![density_string.as_str()]
    } else {
        vec!["reset"]
    };
    run_wm_density(&args).map(|_| ())
}

fn restore_density(original_density: Option<u32>) -> anyhow::Result<()> {
    set_density(original_density)
}

fn stop_watcher(watcher_pid: i32) {
    if watcher_pid <= 0 {
        return;
    }

    unsafe {
        // The watcher calls setsid(), so its process group also contains any
        // in-flight command child. Stop the whole isolated group before the
        // caller restores the session state.
        if libc::kill(-watcher_pid, libc::SIGTERM) != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                warn!("Failed to stop watcher {watcher_pid}: {error}");
            }
        }

        wait_for_watcher(watcher_pid);
    }
}

fn wait_for_watcher(watcher_pid: i32) {
    if watcher_pid <= 0 {
        return;
    }

    unsafe {
        loop {
            let result = libc::waitpid(watcher_pid, std::ptr::null_mut(), 0);
            if result == watcher_pid {
                break;
            }
            if result < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                // ECHILD means reap_zombie_watchers() already collected it.
                break;
            }
        }
    }
}

fn spawn_process_state_watcher(pid: u32, action: WatcherAction) -> anyhow::Result<i32> {
    let spawn_label = action.spawn_label();

    unsafe {
        match libc::fork() {
            -1 => anyhow::bail!("fork failed: {}", std::io::Error::last_os_error()),
            0 => {
                if libc::setsid() == -1 {
                    libc::_exit(1);
                }
                let label = action.label();
                if let Err(e) = watch_process_state(pid, action) {
                    error!("{label} failed for pid {pid}: {e}");
                }
                libc::_exit(0);
            }
            child_pid => {
                info!("Spawned {spawn_label} pid={child_pid} for app pid={pid}");
                Ok(child_pid)
            }
        }
    }
}

fn watch_process_state(pid: u32, action: WatcherAction) -> anyhow::Result<()> {
    // 优先使用 inotify 监听 oom_score_adj（事件驱动，零轮询）。
    // 回退到 /proc/<pid>/cgroup 轮询（inotify 在部分设备/内核上不可用）。
    match watch_via_inotify(pid, &action) {
        Ok(()) => return Ok(()),
        Err(e) => {
            warn!(
                "inotify on oom_score_adj unavailable for {} ({e}), falling back to cgroup polling",
                action.label()
            );
        }
    }

    watch_via_cgroup_polling(pid, &action)
}

fn watch_via_inotify(pid: u32, action: &WatcherAction) -> anyhow::Result<()> {
    const BACKGROUND_THRESHOLD: i32 = 200;
    const BACKGROUND_DEBOUNCE: Duration = Duration::from_secs(2);

    let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0u32) };
    if pidfd < 0 {
        anyhow::bail!("pidfd_open failed");
    }
    let pidfd = pidfd as i32;

    let ifd = unsafe { libc::inotify_init() };
    if ifd < 0 {
        unsafe { libc::close(pidfd) };
        anyhow::bail!("inotify_init failed");
    }

    let oom_path = format!("/proc/{pid}/oom_score_adj\0");
    let wd = unsafe {
        libc::inotify_add_watch(
            ifd,
            oom_path.as_ptr() as *const libc::c_char,
            libc::IN_MODIFY,
        )
    };
    if wd < 0 {
        unsafe {
            libc::close(ifd);
            libc::close(pidfd);
        }
        anyhow::bail!("inotify_add_watch on oom_score_adj failed");
    }
    let wd = wd as u32;

    let efd = unsafe { libc::epoll_create1(0) };
    if efd < 0 {
        unsafe {
            libc::inotify_rm_watch(ifd, wd);
            libc::close(ifd);
            libc::close(pidfd);
        }
        anyhow::bail!("epoll_create1 failed");
    }

    let mut ev = libc::epoll_event {
        events: libc::EPOLLIN as u32,
        u64: pidfd as u64,
    };
    unsafe { libc::epoll_ctl(efd, libc::EPOLL_CTL_ADD, pidfd, &mut ev) };
    ev.u64 = ifd as u64;
    unsafe { libc::epoll_ctl(efd, libc::EPOLL_CTL_ADD, ifd, &mut ev) };

    let label = action.label();
    let mut is_applied = true;
    let mut background_since: Option<Instant> = None;
    let mut events = [libc::epoll_event { events: 0, u64: 0 }; 2];

    info!("{label}: inotify monitoring oom_score_adj for pid {pid}");

    loop {
        let timeout = if let Some(bg_start) = background_since {
            BACKGROUND_DEBOUNCE
                .checked_sub(bg_start.elapsed())
                .unwrap_or(Duration::ZERO)
                .as_millis() as i32
        } else {
            -1
        };

        let nfds = unsafe { libc::epoll_wait(efd, events.as_mut_ptr(), 2, timeout) };

        if let Some(bg_start) = background_since
            && bg_start.elapsed() >= BACKGROUND_DEBOUNCE
        {
            if is_applied {
                action.restore()?;
                is_applied = false;
                info!("{label} restored session for pid {pid}");
            }
            background_since = None;
        }

        if nfds < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            warn!("{label}: epoll_wait error: {error}, attempting restore before exit");
            if is_applied {
                let _ = action.restore();
            }
            break;
        }

        if nfds == 0 {
            continue;
        }

        let process_exited = events
            .iter()
            .take(nfds as usize)
            .any(|event| event.u64 == pidfd as u64);
        if process_exited {
            if is_applied {
                action.restore()?;
            }
            info!("{label}: app pid {pid} exited (pidfd event)");
            break;
        }

        for event in events.iter().take(nfds as usize) {
            if event.u64 != ifd as u64 {
                continue;
            }

            let mut buf = [0u8; 512];
            let _ = unsafe { libc::read(ifd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            let oom_value = read_oom_score_adj(pid);
            if oom_value >= BACKGROUND_THRESHOLD {
                let bg_start = *background_since.get_or_insert_with(Instant::now);
                if is_applied && bg_start.elapsed() >= BACKGROUND_DEBOUNCE {
                    action.restore()?;
                    is_applied = false;
                    background_since = None;
                    info!("{label} restored session for pid {pid} (oom={oom_value})");
                }
            } else {
                background_since = None;
                if !is_applied {
                    action.apply()?;
                    is_applied = true;
                    info!("{label} re-applied session for pid {pid} (oom={oom_value})");
                }
            }
        }
    }

    unsafe {
        libc::epoll_ctl(efd, libc::EPOLL_CTL_DEL, ifd, std::ptr::null_mut());
        libc::epoll_ctl(efd, libc::EPOLL_CTL_DEL, pidfd, std::ptr::null_mut());
        libc::inotify_rm_watch(ifd, wd);
        libc::close(efd);
        libc::close(ifd);
        libc::close(pidfd);
    }
    Ok(())
}

/// 读取 /proc/<pid>/oom_score_adj，失败返回 0（视为前台）。
fn read_oom_score_adj(pid: u32) -> i32 {
    let path = format!("/proc/{pid}/oom_score_adj");
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .unwrap_or(0)
}

/// 轮询回退方案：/proc/<pid>/cgroup 检查 top-app（与原实现相同）。
fn watch_via_cgroup_polling(pid: u32, action: &WatcherAction) -> anyhow::Result<()> {
    const POLL_INTERVAL: Duration = Duration::from_millis(200);
    const BACKGROUND_DEBOUNCE: Duration = Duration::from_secs(2);

    let proc_path = format!("/proc/{pid}");
    let label = action.label();
    let mut is_applied = true;
    let mut background_since: Option<Instant> = None;

    info!("{label}: cgroup polling for pid {pid}");

    loop {
        if !std::path::Path::new(&proc_path).exists() {
            if is_applied {
                action.restore()?;
            }
            break;
        }

        if is_process_in_top_app(pid) {
            background_since = None;
            if !is_applied {
                action.apply()?;
                is_applied = true;
                info!("{label} re-applied session for pid {pid}");
            }
        } else {
            let bg_start = background_since.get_or_insert_with(Instant::now);
            if is_applied && bg_start.elapsed() >= BACKGROUND_DEBOUNCE {
                action.restore()?;
                is_applied = false;
                background_since = None;
                info!("{label} restored session for pid {pid}");
            }
        }

        thread::sleep(POLL_INTERVAL);
    }

    Ok(())
}

fn restore_properties(request: RestoreRequest) -> anyhow::Result<()> {
    if request.props.is_empty() {
        return Ok(());
    }

    for (key, value) in &request.props {
        apply_resetprop(key, value)?;
    }

    // Rebuild after restoring originals to reclaim any holes.
    rebuild_all_contexts(request.props.keys());

    Ok(())
}

fn backup_property(key: &str) -> anyhow::Result<String> {
    let output = std::process::Command::new("getprop").arg(key).output()?;
    if !output.status.success() {
        anyhow::bail!("getprop failed for {key}");
    }

    let value = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\n', '\r'])
        .to_string();
    Ok(value)
}

fn new_resetprop() -> anyhow::Result<ResetProp> {
    sys_prop::init()
        .map_err(|e| anyhow::anyhow!("failed to initialize system property API: {e}"))?;

    Ok(ResetProp {
        // `-n`: bypass property_service, direct mmap write.
        // All properties we set (ro.*, persist.*, etc.) benefit from direct
        // mmap — no SELinux policy denials, no init service restarts, no
        // PROP_VALUE_MAX limit.  ro.* is forced to mmap regardless, but
        // skip_svc=true also covers non-ro keys in custom_props.
        skip_svc: true,
        persistent: false,
        persist_only: false,
        verbose: false,
        show_context: false,
        rebuild: false,
    })
}

fn apply_resetprop(key: &str, value: &str) -> anyhow::Result<()> {
    let rp = new_resetprop()?;

    if let Err(e) = rp.set(key, value) {
        // 值超过 PROP_VALUE_MAX 时，inline prop_info 无法原地扩展。
        // 先删除旧属性（释放 inline 空间），再重新创建为 long 模式。
        warn!("resetprop set failed for {key}, trying delete+set: {e}");
        let _ = rp.delete(key);
        rp.set(key, value)
            .map_err(|e2| anyhow::anyhow!("resetprop delete+set failed for {key}: {e2}"))?;
    }
    Ok(())
}

fn resetprop_delete(key: &str) -> anyhow::Result<()> {
    let rp = new_resetprop()?;

    match rp.delete(key) {
        Ok(true) => Ok(()),
        // 属性族展开后，删除列表包含设备上可能不存在的分区副本
        // （如旧设备没有 system_dlkm/vendor_dlkm 构建 props）；
        // 本来就不存在的属性视为已删除，避免整个 Apply 会话失败回滚。
        Ok(false) => {
            info!("resetprop delete: '{key}' not present, treating as deleted");
            Ok(())
        }
        Err(_) => anyhow::bail!("resetprop delete failed for {key}"),
    }
}

fn apply_props_batch(
    props: &HashMap<String, String>,
    delete_props: &[String],
) -> anyhow::Result<()> {
    for (key, value) in props {
        apply_resetprop(key, value)?;
    }

    for key in delete_props {
        resetprop_delete(key)?;
    }

    rebuild_all_contexts(props.keys().chain(delete_props.iter()));

    Ok(())
}

fn restore_props_batch(backups: &[PropBackup]) -> anyhow::Result<()> {
    for entry in backups {
        apply_resetprop(&entry.key, &entry.original_value)?;
    }

    // Rebuild using the first backup's key to find the context.
    rebuild_all_contexts(backups.iter().map(|b| &b.key));

    Ok(())
}

const LOG_PATH: &str = "/data/adb/device_faker/logs/device_faker.log";

fn write_log_lines(request: WriteLogRequest) -> anyhow::Result<()> {
    if request.lines.is_empty() {
        return Ok(());
    }

    write_log_lines_to_path(LOG_PATH, &request.lines)
}

fn write_log_lines_to_path(path: &str, lines: &[String]) -> anyhow::Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;

    for line in lines {
        writeln!(file, "{line}")?;
    }

    file.flush()?;
    Ok(())
}

fn is_process_in_top_app(pid: u32) -> bool {
    let cgroup_path = format!("/proc/{pid}/cgroup");
    match fs::read_to_string(&cgroup_path) {
        Ok(content) => content.lines().any(|line| line.contains("top-app")),
        Err(_) => true,
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct ResetpropSessionRequest {
    pid: u32,
    props: HashMap<String, String>,
    delete_props: Vec<String>,
    package_name: String,
    #[serde(default)]
    density: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct RestoreRequest {
    props: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "cmd", content = "payload")]
pub enum CompanionRequest {
    Apply(ResetpropSessionRequest),
    Restore(RestoreRequest),
    CpuSpoof(CpuSpoofRequest),
    CpuSpoofUnmount(CpuSpoofUnmountRequest),
    WriteLog(WriteLogRequest),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CompanionResponse {
    pub status: i32,
    pub message: Option<String>,
    pub backups: Option<HashMap<String, String>>,
}

impl CompanionResponse {
    pub fn ok() -> Self {
        Self {
            status: 0,
            message: None,
            backups: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            status: -1,
            message: Some(msg.into()),
            backups: None,
        }
    }

    pub fn ok_with_backups(backups: HashMap<String, String>) -> Self {
        Self {
            status: 0,
            message: None,
            backups: Some(backups),
        }
    }
}

#[derive(Clone)]
struct PropBackup {
    key: String,
    original_value: String,
}
