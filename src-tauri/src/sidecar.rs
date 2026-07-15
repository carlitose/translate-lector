//! Lifecycle of the app-managed llama-server (tickets 03/04, decisions D1/D4/D5).
//!
//! The app spawns `llama-server` **on-demand** (at the first local translation,
//! not at launch — D5) and kills it deterministically when the app exits
//! (`RunEvent::Exit | ExitRequested`, D1). Only the direct llama.cpp provider
//! (`llamaserver`, the one carrying a configured `binary_path`) is app-managed;
//! the other local providers (`unsloth`/`lmstudio`/`ollama`) are launched by the
//! user and must keep their pre-ticket behaviour.
//!
//! ## Spawn mechanism — `command-group` (std) with per-OS lifetime binding (ticket 10)
//!
//! We spawn an **external** binary at a user-configured absolute path (D0/D1: no
//! bundling, no target-triple sidecar) with a plain blocking
//! `std::process::Command` (this path already runs inside `spawn_blocking`),
//! wrapped by the [`command-group`](https://docs.rs/command-group) crate so the
//! child's lifetime is tied to the app and cannot be orphaned:
//!
//! - **Windows** — kernel-guaranteed. `.kill_on_drop(true)` makes the crate
//!   create the child inside a Job Object with
//!   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The app process is the sole holder of
//!   the job handle, so when it dies for *any* reason — clean exit, panic,
//!   crash, Task Manager force-kill — the kernel closes the handle and
//!   terminates every process in the job. The orphan hole is closed by the OS,
//!   not by our code.
//! - **macOS / Linux (Unix)** — reliable *explicit* tree-kill. The child is made
//!   a process-group leader (`setpgid(0,0)`), so the kill on exit
//!   ([`GroupChild::kill`] → `killpg(SIGKILL)`) tears down the whole tree. A
//!   *hard crash* of the app does **not** auto-kill the child on Unix (no
//!   Job-Object equivalent; a kqueue `NOTE_EXIT` watchdog is out of scope); that
//!   case stays covered by [`reap_stale_llama_server_on_startup`] as before.
//!
//! Why `command-group` and not `process-wrap`: both are by the same author, but
//! `process-wrap`'s **std** `JobObject` hard-codes the job flag to `false`
//! (kill-on-close only reachable via its *tokio* API, which would force a tokio
//! runtime context onto this blocking spawn path). `command-group`'s std builder
//! exposes `.kill_on_drop(true)` directly, so it compiles cleanly here *and*
//! guarantees kill-on-parent-death on Windows — the deciding criterion.
//!
//! Dropping `tauri-plugin-shell` loses its `CommandEvent` (stdout) stream, which
//! is fine: readiness is detected over HTTP (`probe_reachable`/`probe_model_ready`
//! on `/health`), never by parsing logs. The child's stdio is sent to
//! `Stdio::null()` so the chatty server can never block on a full pipe.
//!
//! ## Testability
//!
//! The *decisions* are pure functions ([`spawn_decision`], [`build_llama_args`],
//! [`reap_decision`], [`is_managed_local_provider`], [`binary_image_name`]) with
//! unit tests below; none spawn a real process. The imperative shell (spawn /
//! poll / kill / reap) is a thin wrapper around them.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use command_group::{CommandGroup, GroupChild};
use tauri::Manager;

/// `CREATE_NO_WINDOW` (winbase.h): keep console-mode child processes
/// (llama-server.exe, tasklist, taskkill) from flashing a console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What the on-demand path should do for a translation request, given the
/// provider's locality, current reachability and whether its configured paths
/// are valid. Pure; the caller performs the matching side effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnAction {
    /// Remote (cloud) provider — never spawn; just translate.
    SkipRemote,
    /// Local and already answering the probe — reuse it (no double-spawn).
    ReuseExisting,
    /// Local, not answering, paths valid — spawn the server.
    Spawn,
    /// Local, not answering, paths missing/invalid — surface the actionable
    /// path error instead of an opaque spawn (D2).
    ErrorMissingPaths,
}

/// Decide spawn-vs-reuse-vs-error for an **app-managed** local provider.
/// `is_local` distinguishes local from cloud; `reachable` is the result of the
/// existing `probe_reachable`; `paths_ok` is whether `validate_llama_paths`
/// succeeded. Pure — no I/O.
pub fn spawn_decision(is_local: bool, reachable: bool, paths_ok: bool) -> SpawnAction {
    if !is_local {
        return SpawnAction::SkipRemote;
    }
    if reachable {
        return SpawnAction::ReuseExisting;
    }
    if paths_ok {
        SpawnAction::Spawn
    } else {
        SpawnAction::ErrorMissingPaths
    }
}

/// Whether a provider is one the **app manages** (spawns/kills). True only for a
/// local (loopback) provider that declares a `binary_path` — i.e. the direct
/// llama.cpp `llamaserver` preset. Cloud providers and the other local presets
/// (Unsloth/LM Studio/Ollama, which the user runs) are not managed: the app must
/// not try to spawn them nor emit a "binary missing" error for them. Pure.
pub fn is_managed_local_provider(is_local: bool, binary_path_blank: bool) -> bool {
    is_local && !binary_path_blank
}

/// Build the llama-server argument list for the D4 defaults (all overridable via
/// the provider config that supplies `port`/`n_ctx`/`model_path`):
/// `-m <model> --port <port> -ngl 99 -c <n_ctx> --reasoning off --parallel 1`.
/// Pure and unit-tested for the exact flags D4 pins down.
pub fn build_llama_args(port: u16, n_ctx: u32, model_path: &str) -> Vec<String> {
    vec![
        "-m".to_string(),
        model_path.to_string(),
        "--port".to_string(),
        port.to_string(),
        "-ngl".to_string(),
        "99".to_string(),
        "-c".to_string(),
        n_ctx.to_string(),
        "--reasoning".to_string(),
        "off".to_string(),
        "--parallel".to_string(),
        "1".to_string(),
    ]
}

/// Resolve the concrete TCP port the managed server must both spawn on (`--port`)
/// and be probed on, from the provider `base_url`. Fails fast (FIX B) with an
/// actionable Italian message when the base_url carries no explicit port: were we
/// to spawn on a hardcoded default while probing the scheme default (`:80`), the
/// two would disagree and the readiness poll would hang ~30 s before reporting a
/// misleading error. The shipped preset (`http://127.0.0.1:8080/...`) has an
/// explicit port, so it takes the happy path unchanged. Pure — no I/O.
pub fn resolve_spawn_port(base_url: &str) -> Result<u16, String> {
    crate::llm::port_from_base_url(base_url).ok_or_else(|| {
        "base_url del provider locale senza porta esplicita: impostala in ⚙️".to_string()
    })
}

/// What startup orphan-reaping (assumption 2) should do, given the PID persisted
/// by a previous run and whether that PID is currently alive as our server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReapAction {
    /// A recorded server PID is still alive — terminate it.
    Reap(u32),
    /// Nothing to reap (no PID recorded, PID `0`, or the process is gone).
    Nothing,
}

/// Decide whether to reap a stale server. Conservative: reap only a concrete,
/// non-zero PID that is confirmed still alive (as our binary image — the caller
/// injects that check via `pid_alive`). This never touches a server the user
/// launched manually because we only ever consider a PID **we recorded**. Pure.
pub fn reap_decision(persisted_pid: Option<u32>, pid_alive: bool) -> ReapAction {
    match persisted_pid {
        Some(pid) if pid != 0 && pid_alive => ReapAction::Reap(pid),
        _ => ReapAction::Nothing,
    }
}

/// The process image (file name) of a configured binary path, e.g.
/// `…\llama-server.exe` → `llama-server.exe`. Used as a PID-reuse guard for
/// reaping. `None` for a blank path. Pure.
pub fn binary_image_name(binary_path: &str) -> Option<String> {
    let p = binary_path.trim();
    if p.is_empty() {
        return None;
    }
    Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
}

// --- Managed-process state ---------------------------------------------------

/// Tauri-managed handle to the app-spawned llama-server. Held in a `Mutex`
/// (recovered on poison via `crate::lock_ignoring_poison`) next to
/// `CurrentPage`/`LocalProviderSlot`. `None` until the first on-demand spawn;
/// `.take()` + `.kill()` on exit (D1). The [`GroupChild`] owns the OS handle
/// that binds the child's lifetime to the app — on Windows keeping it alive here
/// keeps the Job Object's `KILL_ON_JOB_CLOSE` armed until the app dies (ticket
/// 10). `GroupChild` is `Send + Sync` (its Windows `JobPort` is explicitly so),
/// which is what lets it live in Tauri-managed state.
pub struct LlamaServerProcess(pub Mutex<Option<GroupChild>>);

impl LlamaServerProcess {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }
}

/// Bounded readiness wait after a spawn: `probe_model_ready` (1.5 s each) is
/// polled until the model is loaded (llama.cpp `/health` → 200) or this deadline
/// elapses.
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(30);
/// Pause between readiness probes.
const SERVER_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Ensure the app-managed llama-server is up before a local translation runs.
///
/// - Cloud or non-managed local provider → `Ok(())` (no-op; the normal translate
///   path handles reachability as before).
/// - Managed + already reachable → `Ok(())` (reuse; **no double-spawn**).
/// - Managed + not reachable + paths valid → spawn with the D4 args and poll
///   `probe_model_ready` (llama.cpp `/health` → 200, i.e. model loaded, not just
///   the socket) until ready (bounded ~30 s).
/// - Managed + not reachable + paths invalid → the actionable path error (D2).
///
/// Must be called from a blocking context (it sleeps and does blocking probes);
/// in `translate_page` it runs inside `spawn_blocking`, serialized by
/// `LocalProviderSlot`, so two concurrent local requests cannot double-spawn.
pub fn ensure_local_server_ready(
    app: &tauri::AppHandle,
    cfg: &crate::settings::ProviderConfig,
) -> Result<(), String> {
    let is_local = crate::llm::is_local_url(&cfg.base_url);
    if !is_managed_local_provider(is_local, cfg.binary_path.trim().is_empty()) {
        return Ok(());
    }

    let reachable = crate::llm::probe_reachable(&cfg.base_url);
    // FIX D: validate the paths ONCE and keep the `Result`; branch on it below so
    // there is no redundant second fs stat and no TOCTOU where a re-run `.err()`
    // could come back `None` and silently fall back to a "missing" message.
    let paths = crate::settings::validate_llama_paths(&cfg.binary_path, &cfg.model_path);

    // FIX F: `is_local` is guaranteed true here (the `is_managed_local_provider`
    // guard above already returned for cloud), so `SkipRemote` is unreachable
    // from this call site — it is retained only for the standalone truth-table
    // tests of `spawn_decision`.
    match spawn_decision(is_local, reachable, paths.is_ok()) {
        SpawnAction::SkipRemote | SpawnAction::ReuseExisting => Ok(()),
        SpawnAction::ErrorMissingPaths => {
            // Carry the exact actionable Italian message from the single
            // validation above (D2).
            Err(paths
                .err()
                .unwrap_or_else(|| crate::settings::LLAMA_BINARY_MISSING_MSG.to_string()))
        }
        SpawnAction::Spawn => {
            let resolved = paths?;
            // FIX B: resolve the port ONCE (see `resolve_spawn_port`) and drive
            // BOTH the `--port` spawn arg and the readiness probe from it.
            let port = resolve_spawn_port(&cfg.base_url)?;
            let args = build_llama_args(port, cfg.n_ctx, &resolved.model_path.to_string_lossy());
            spawn_llama_server(app, &resolved.binary_path, args)?;
            // The port is now guaranteed explicit in base_url, so probing
            // base_url probes exactly the port we spawned on — they cannot
            // disagree.
            wait_until_ready(&cfg.base_url)
        }
    }
}

/// Spawn `binary` with `args` as a **process group / Job Object** via
/// `command-group` (see the module doc for the per-OS lifetime guarantees),
/// store the [`GroupChild`] in managed state (killing any previously stored
/// child first, to avoid a leak) and persist its PID for the startup-reap
/// fallback. The child's stdio goes to `Stdio::null()`: readiness is detected by
/// HTTP probing, so we neither need nor drain the server's (chatty) output, and
/// with no pipe there is nothing that can fill and block it.
fn spawn_llama_server(
    app: &tauri::AppHandle,
    binary: &Path,
    args: Vec<String>,
) -> Result<(), String> {
    let program = binary.to_string_lossy().to_string();

    let mut command = Command::new(&program);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // `.group()` puts the child in a POSIX process group on Unix / a Job Object
    // on Windows. `.spawn()` needs `&mut self`, so `builder` is always mutated.
    let mut builder = command.group();
    #[cfg(windows)]
    {
        // `CREATE_NO_WINDOW`: keep the console-mode llama-server.exe from
        // flashing a console window (parity with the old shell-plugin spawn).
        // Set via the builder (not the raw `Command`) because command-group
        // overwrites the creation flags to add `CREATE_SUSPENDED` internally.
        builder.creation_flags(CREATE_NO_WINDOW);
        // Arm `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: the kernel kills the whole
        // job when the app (sole handle holder) dies for ANY reason — clean
        // exit, panic, crash, or force-kill. This is the ticket-10 fix that
        // closes the orphan hole on Windows. The method is Windows-only in
        // command-group's std API, hence the `#[cfg(windows)]`.
        builder.kill_on_drop(true);
    }
    let child = builder
        .spawn()
        .map_err(|e| format!("Impossibile avviare llama-server ({program}): {e}"))?;

    let pid = child.id();
    if let Ok(pid_file) = llama_pid_file_path(app) {
        let _ = std::fs::write(&pid_file, pid.to_string());
    }

    let state = app.state::<LlamaServerProcess>();
    let mut guard = crate::lock_ignoring_poison(&state.0);
    if let Some(mut old) = guard.take() {
        let _ = old.kill();
    }
    *guard = Some(child);

    Ok(())
}

/// Poll `probe_model_ready(base_url)` until the freshly spawned server has the
/// **model loaded** (llama.cpp `/health` → 200) or [`SERVER_READY_TIMEOUT`]
/// elapses. Waiting on model-readiness rather than bare socket reachability
/// (`probe_reachable`) is the ticket-08 fix: llama.cpp answers `/v1/models` (so
/// `probe_reachable` is already `true`) while `chat/completions` still returns
/// `503 "Loading model"`, so probing the socket let the first cold translation
/// hit a 503. `/health` stays `503` until the model is in VRAM, so this loop only
/// returns `Ok` once the very first translation can actually succeed. On timeout,
/// an actionable message (not the generic "non raggiungibile").
fn wait_until_ready(base_url: &str) -> Result<(), String> {
    let deadline = Instant::now() + SERVER_READY_TIMEOUT;
    loop {
        if crate::llm::probe_model_ready(base_url) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(
                "Il server llama.cpp è stato avviato ma non ha risposto in tempo. \
                 Verifica il path del modello in ⚙️ o che la porta non sia occupata."
                    .to_string(),
            );
        }
        std::thread::sleep(SERVER_POLL_INTERVAL);
    }
}

/// Kill the app-managed llama-server on exit: `.take()` the [`GroupChild`] from
/// managed state and `.kill()` it — on Windows this `TerminateJobObject`s the
/// whole job, on Unix `killpg(SIGKILL)`s the whole process group — then remove
/// the PID file (a clean shutdown leaves no orphan to reap). Called from the
/// `RunEvent::Exit | ExitRequested` callback (D1). On Windows the OS would also
/// reap the child on its own (KILL_ON_JOB_CLOSE) when the handle drops, but this
/// explicit kill is the deterministic path for a clean shutdown on every OS.
pub fn kill_llama_server_on_exit(app: &tauri::AppHandle) {
    let child = {
        let state = app.state::<LlamaServerProcess>();
        let mut guard = crate::lock_ignoring_poison(&state.0);
        guard.take()
    };
    if let Some(mut child) = child {
        match child.kill() {
            Ok(()) => eprintln!("[llama] server killed on exit"),
            Err(e) => eprintln!("[llama] failed to kill server on exit: {e}"),
        }
    }
    if let Ok(pid_file) = llama_pid_file_path(app) {
        let _ = std::fs::remove_file(pid_file);
    }
}

/// Reap a stale app-managed llama-server left running by a prior **hard crash**
/// (assumption 2): `RunEvent` never fired, so [`kill_llama_server_on_exit`] did
/// not run and the PID file still points at a live process. Called from `setup`
/// before any on-demand spawn.
///
/// Conservative by construction — it only ever considers the PID **we recorded**
/// and, on Windows, only reaps it when a live process with that PID still has our
/// binary's image name (PID-reuse guard). The image name is derived from the
/// *configured* `llamaserver` `binary_path` (FIX E), so a genuine orphan of a
/// user-renamed binary is still reaped; only when the config cannot be read do we
/// fall back to the default path's image name. Limitations (best-effort,
/// documented): a PID reused by another `llama-server.exe` in the tiny
/// crash-to-relaunch window could be reaped (rare, and it would itself be a
/// llama-server). Perfect detection (Job Objects) is out of scope for a
/// personal-use build.
///
/// SINGLE-INSTANCE ASSUMPTION (FIX G / follow-up): the app assumes only one
/// instance runs at a time. The PID file is shared, so launching a *second*
/// instance would reap the *first* instance's app-managed server. The intended
/// future fix is to register `tauri-plugin-single-instance` (not added now).
pub fn reap_stale_llama_server_on_startup(app: &tauri::AppHandle) {
    let pid_file = match llama_pid_file_path(app) {
        Ok(p) => p,
        Err(_) => return,
    };
    let persisted = read_pid_file(&pid_file);
    // FIX E: guard against PID reuse with the image name of the *configured*
    // binary (override-aware), falling back to the default only if config is
    // unavailable.
    let image = active_llamaserver_image_name(app)
        .or_else(|| binary_image_name(crate::settings::DEFAULT_LLAMASERVER_BINARY_PATH));
    let alive = match (persisted, image.as_deref()) {
        (Some(pid), Some(img)) => pid_matches_running_image(pid, img),
        _ => false,
    };
    match reap_decision(persisted, alive) {
        ReapAction::Reap(pid) => {
            eprintln!("[llama] reaping stale server pid={pid} left by a prior hard crash");
            kill_pid(pid);
            let _ = std::fs::remove_file(&pid_file);
        }
        ReapAction::Nothing => {
            // Dead/reused/absent PID: drop the stale pointer so it is not
            // reconsidered on the next launch.
            let _ = std::fs::remove_file(&pid_file);
        }
    }
}

/// Image name (e.g. `llama-server.exe`) of the **configured** `llamaserver`
/// provider's `binary_path`, resolving any user override via
/// [`crate::settings::get_provider_config`] (same source used everywhere else).
/// `None` when the DB/config cannot be read or the path is blank — the caller
/// then falls back to the default path's image name. Best-effort by design: any
/// failure downgrades to the default rather than widening what gets reaped.
fn active_llamaserver_image_name(app: &tauri::AppHandle) -> Option<String> {
    let db_path = crate::database_path(app).ok()?;
    let conn = crate::db::open_and_init(&db_path).ok()?;
    let cfg = crate::settings::get_provider_config(&conn, "llamaserver").ok()?;
    binary_image_name(&cfg.binary_path)
}

/// Absolute path of the PID file recording the app-spawned server, kept in the OS
/// app-config dir (survives a data-folder relocation). Creates the dir on demand.
fn llama_pid_file_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("cannot resolve app config dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create app config dir: {e}"))?;
    Ok(dir.join("llama-server.pid"))
}

/// Read the recorded server PID, or `None` when absent/blank/unparsable.
fn read_pid_file(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse::<u32>().ok()
}

/// Whether a live process with `pid` currently has image name `image` (Windows).
/// Best-effort via `tasklist`; any error is treated as "not running" (no reap).
#[cfg(windows)]
fn pid_matches_running_image(pid: u32, image: &str) -> bool {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("tasklist")
        .args([
            "/FI",
            &format!("PID eq {pid}"),
            "/FI",
            &format!("IMAGENAME eq {image}"),
            "/NH",
            "/FO",
            "CSV",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .to_ascii_lowercase()
            .contains(&image.to_ascii_lowercase()),
        Err(_) => false,
    }
}

/// Non-Windows best-effort: no cheap image-checked liveness probe wired up, so we
/// conservatively report "not our server" and skip the reap.
#[cfg(not(windows))]
fn pid_matches_running_image(_pid: u32, _image: &str) -> bool {
    false
}

/// Forcibly terminate `pid` (Windows `taskkill /F /PID`). Best-effort.
#[cfg(windows)]
fn kill_pid(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

/// Non-Windows best-effort no-op (paired with the conservative liveness probe).
#[cfg(not(windows))]
fn kill_pid(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    // --- spawn_decision ------------------------------------------------------

    #[test]
    fn spawn_decision_skips_remote_regardless_of_the_rest() {
        // Cloud: never spawn, even if it happens to be "unreachable".
        assert_eq!(spawn_decision(false, false, false), SpawnAction::SkipRemote);
        assert_eq!(spawn_decision(false, true, true), SpawnAction::SkipRemote);
    }

    #[test]
    fn spawn_decision_reuses_a_reachable_local_server() {
        // Healthy server already on the port → reuse (no double-spawn, AC).
        assert_eq!(spawn_decision(true, true, true), SpawnAction::ReuseExisting);
        // Reuse wins even if paths look unset — a live server is a live server.
        assert_eq!(spawn_decision(true, true, false), SpawnAction::ReuseExisting);
    }

    #[test]
    fn spawn_decision_spawns_when_local_down_and_paths_ok() {
        assert_eq!(spawn_decision(true, false, true), SpawnAction::Spawn);
    }

    #[test]
    fn spawn_decision_errors_when_local_down_and_paths_missing() {
        assert_eq!(spawn_decision(true, false, false), SpawnAction::ErrorMissingPaths);
    }

    // --- is_managed_local_provider -------------------------------------------

    #[test]
    fn is_managed_only_for_a_local_provider_with_a_configured_binary() {
        // llamaserver: local + configured binary → managed.
        assert!(is_managed_local_provider(true, false));
        // Cloud → never managed.
        assert!(!is_managed_local_provider(false, false));
        assert!(!is_managed_local_provider(false, true));
        // Unsloth/LM Studio/Ollama: local but no configured binary → not managed
        // (the user launches those; the app must not spawn them nor error).
        assert!(!is_managed_local_provider(true, true));
    }

    // --- build_llama_args ----------------------------------------------------

    #[test]
    fn build_llama_args_pins_the_d4_flags() {
        let args = build_llama_args(8080, 4096, r"C:\models\gemma.gguf");
        // Adjacency matters: each flag must be immediately followed by its value.
        let has_pair = |flag: &str, val: &str| {
            args.windows(2).any(|w| w[0] == flag && w[1] == val)
        };
        assert!(has_pair("-m", r"C:\models\gemma.gguf"), "model path passed via -m");
        assert!(has_pair("--port", "8080"), "port wired from base_url");
        assert!(has_pair("-ngl", "99"), "full GPU offload (D4)");
        assert!(has_pair("-c", "4096"), "ctx-size wired from n_ctx (D4)");
        assert!(has_pair("--reasoning", "off"), "suppress CoT — the point of the map (D4)");
        assert!(has_pair("--parallel", "1"), "single slot (D4)");
    }

    #[test]
    fn build_llama_args_threads_port_and_ctx_through() {
        let args = build_llama_args(1234, 8192, "/m.gguf");
        let has_pair = |flag: &str, val: &str| {
            args.windows(2).any(|w| w[0] == flag && w[1] == val)
        };
        assert!(has_pair("--port", "1234"));
        assert!(has_pair("-c", "8192"));
        // Constant flags stay put regardless of port/ctx.
        assert!(has_pair("-ngl", "99"));
        assert!(has_pair("--parallel", "1"));
    }

    // --- resolve_spawn_port (FIX B) ------------------------------------------

    #[test]
    fn resolve_spawn_port_returns_the_explicit_port() {
        // Shipped preset and any base_url with an explicit port: happy path,
        // used for BOTH the --port arg and the probe target.
        assert_eq!(resolve_spawn_port("http://127.0.0.1:8080/v1/chat/completions"), Ok(8080));
        assert_eq!(resolve_spawn_port("http://localhost:1234/v1"), Ok(1234));
        assert_eq!(resolve_spawn_port("http://[::1]:5000/v1"), Ok(5000));
    }

    #[test]
    fn resolve_spawn_port_fails_fast_without_an_explicit_port() {
        // Portless local base_url: fail fast with the actionable message rather
        // than spawn on a default while probing :80 (a 30 s misleading hang).
        let err = resolve_spawn_port("http://127.0.0.1/v1/chat/completions").unwrap_err();
        assert_eq!(err, "base_url del provider locale senza porta esplicita: impostala in ⚙️");
        assert!(resolve_spawn_port("http://localhost/v1").is_err());
    }

    // --- reap_decision -------------------------------------------------------

    #[test]
    fn reap_decision_reaps_a_live_recorded_pid() {
        assert_eq!(reap_decision(Some(4321), true), ReapAction::Reap(4321));
    }

    #[test]
    fn reap_decision_does_nothing_when_the_recorded_pid_is_gone() {
        // Clean prior shutdown / dead process → nothing to reap.
        assert_eq!(reap_decision(Some(4321), false), ReapAction::Nothing);
    }

    #[test]
    fn reap_decision_does_nothing_without_a_recorded_pid() {
        assert_eq!(reap_decision(None, true), ReapAction::Nothing);
        assert_eq!(reap_decision(None, false), ReapAction::Nothing);
    }

    #[test]
    fn reap_decision_ignores_a_zero_pid() {
        // PID 0 is never a real user process to reap.
        assert_eq!(reap_decision(Some(0), true), ReapAction::Nothing);
    }

    // --- binary_image_name ---------------------------------------------------

    #[test]
    fn binary_image_name_takes_the_file_name() {
        assert_eq!(
            binary_image_name(r"C:\Users\x\.translate-lector\llama.cpp\llama-server.exe"),
            Some("llama-server.exe".to_string())
        );
        assert_eq!(
            binary_image_name("/usr/local/bin/llama-server"),
            Some("llama-server".to_string())
        );
    }

    #[test]
    fn binary_image_name_is_none_for_a_blank_path() {
        assert_eq!(binary_image_name(""), None);
        assert_eq!(binary_image_name("   "), None);
    }
}
