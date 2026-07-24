use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mindgate_common::{socket_path, wire, Request, Response};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// MindGate's visual identity: a small set of raw-ANSI helpers.
/// Respects https://no-color.org
mod theme {
    use std::io::IsTerminal;

    const BOLD_PINK: &str = "\x1b[1;38;5;213m";
    const DIM: &str = "\x1b[2m";
    const GREEN: &str = "\x1b[32m";
    const RED: &str = "\x1b[31m";
    const YELLOW: &str = "\x1b[33m";
    const RESET: &str = "\x1b[0m";

    fn color_enabled() -> bool {
        std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
    }

    pub fn bold_pink(s: &str) -> String {
        if color_enabled() { format!("{BOLD_PINK}{s}{RESET}") } else { s.to_string() }
    }
    pub fn dim(s: &str) -> String {
        if color_enabled() { format!("{DIM}{s}{RESET}") } else { s.to_string() }
    }
    pub fn ok(s: &str) -> String {
        if color_enabled() { format!("{GREEN}✓ {s}{RESET}") } else { format!("✓ {s}") }
    }
    pub fn warn(s: &str) -> String {
        if color_enabled() { format!("{YELLOW}⚠ {s}{RESET}") } else { format!("⚠ {s}") }
    }
    pub fn err(s: &str) -> String {
        if color_enabled() { format!("{RED}✗ {s}{RESET}") } else { format!("✗ {s}") }
    }
}

#[derive(Parser, Debug)]
#[command(name = "mindgate", about = "Stubborn browser protector for Linux", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Install MindGate (helper for the install script)
    Install,
    /// Uninstall MindGate completely
    Uninstall,
    /// Start the MindGate daemon
    Start,
    /// Stop the MindGate daemon
    Stop,
    /// Restart the MindGate daemon
    Restart,
    /// Check the status of the daemon and extension
    Status,
    /// Run a comprehensive health check of the MindGate setup
    Doctor,
    /// View the daemon logs
    Logs,
    /// Hidden subcommand used by browser extensions to bridge stdio to the daemon socket
    #[command(name = "nativebridge", hide = true)]
    NativeBridge,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Install => {
            enforce_lock_check().await?;
            run_install().await?;
        }
        Commands::Uninstall => {
            enforce_lock_check().await?;
            run_uninstall().await?;
        }
        Commands::Start => {
            // Start is always allowed (if daemon is dead, it can't be locked anyway)
            run_systemctl_command("start", "mindgated.service").await?;
        }
        Commands::Stop => {
            enforce_lock_check().await?;
            let _ = send_shutdown_request().await; // Graceful shutdown
            run_systemctl_command("stop", "mindgated.service").await?;
        }
        Commands::Restart => {
            enforce_lock_check().await?;
            run_systemctl_command("restart", "mindgated.service").await?;
        }
        Commands::Status => run_status().await?,
        Commands::Doctor => run_doctor().await?,
        Commands::Logs => run_logs().await?,
        Commands::NativeBridge => run_native_bridge().await?,
    }

    Ok(())
}

/// Pre-flight check: asks the daemon if it is currently locked.
/// If locked and not expired, rejects the operation cleanly.
async fn enforce_lock_check() -> Result<()> {
    let path = socket_path();
    let mut stream = match UnixStream::connect(&path).await {
        Ok(s) => s,
        Err(_) => return Ok(()), // Daemon not running, so nothing is locked
    };

    let payload = wire::encode(&Request::Status)?;
    let _ = stream.write_all(&payload).await;

    let mut len_buf = [0u8; 4];
    if stream.read_exact(&mut len_buf).await.is_err() {
        return Ok(());
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    if stream.read_exact(&mut body).await.is_err() {
        return Ok(());
    }

    if let Ok(Response::Status(status)) = wire::decode(&body) {
        if let Some(lock) = status.lock_state {
            if lock.locked {
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;

                let is_expired = if let Some(unlock_at) = lock.unlock_at {
                    unlock_at <= now_ms
                } else {
                    false // "forever" lock never expires
                };

                if !is_expired {
                    anyhow::bail!(
                        "{} MindGate is currently locked. Destructive operations are disabled.",
                        theme::err("ERROR:")
                    );
                }
            }
        }
    }
    Ok(())
}

async fn run_systemctl_command(action: &str, unit: &str) -> Result<()> {
    let status = Command::new("systemctl")
        .args([action, unit])
        .status()
        .context("failed to execute systemctl")?;

    if !status.success() {
        anyhow::bail!(
            "systemctl {} {} failed. You may need to run this command with sudo.",
            action,
            unit
        );
    }
    println!("{}", theme::ok(&format!("MindGate {} successfully.", action)));
    Ok(())
}

async fn send_shutdown_request() -> Result<()> {
    let path = socket_path();
    let mut stream = match UnixStream::connect(&path).await {
        Ok(s) => s,
        Err(_) => return Ok(()), // Daemon not running, nothing to shut down gracefully
    };
    let payload = wire::encode(&Request::Shutdown)?;
    let _ = stream.write_all(&payload).await;
    Ok(())
}

async fn run_status() -> Result<()> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .await
        .with_context(|| format!("Could not connect to daemon at {}. Is it running?", path.display()))?;

    let payload = wire::encode(&Request::Status)?;
    stream.write_all(&payload).await?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;

    let response: Response = wire::decode(&body)?;
    match response {
        Response::Status(status) => {
            println!("{}", theme::bold_pink("--- MindGate Status ---"));
            println!("Daemon Running:      {}", if status.daemon_running { theme::ok("YES") } else { theme::err("NO") });
            println!("Extension Connected: {}", if status.extension_connected { theme::ok("YES") } else { theme::err("NO") });
            
            // NEW: Display Lock Status beautifully
            if let Some(lock) = status.lock_state {
                if lock.locked {
                    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
                    let is_expired = if let Some(unlock_at) = lock.unlock_at {
                        unlock_at <= now_ms
                    } else {
                        false
                    };

                    if !is_expired {
                        if let Some(unlock_at) = lock.unlock_at {
                            let remaining_ms = unlock_at.saturating_sub(now_ms);
                            let remaining_secs = remaining_ms / 1000;
                            let hours = remaining_secs / 3600;
                            let minutes = (remaining_secs % 3600) / 60;
                            let seconds = remaining_secs % 60;
                            println!("Lock Status:         {} {}h {}m {}s remaining", theme::err("LOCKED"), hours, minutes, seconds);
                        } else {
                            println!("Lock Status:         {} FOREVER", theme::err("LOCKED"));
                        }
                    } else {
                        println!("Lock Status:         {}", theme::ok("UNLOCKED (Expired)"));
                    }
                } else {
                    println!("Lock Status:         {}", theme::ok("UNLOCKED"));
                }
            } else {
                println!("Lock Status:         {}", theme::ok("UNLOCKED"));
            }
        }
        Response::Error { message } => anyhow::bail!("{message}"),
        _ => anyhow::bail!("Unexpected response from daemon"),
    }
    Ok(())
}

async fn run_doctor() -> Result<()> {
    println!("{}", theme::bold_pink("--- MindGate Doctor ---"));

    // 1. Daemon running
    let daemon_running = check_systemctl_active("mindgated.service").await;
    if daemon_running {
        println!("{}", theme::ok("Daemon running"));
    } else {
        println!("{}", theme::err("Daemon not running"));
    }

    // 2. Watchdog running
    let watchdog_running = check_systemctl_active("mindgate-watchdog.service").await;
    if watchdog_running {
        println!("{}", theme::ok("Watchdog running"));
    } else {
        println!("{}", theme::warn("Watchdog not running"));
    }

    // 3. Native Messaging installed
    let nm_installed = check_native_messaging().await;
    if nm_installed {
        println!("{}", theme::ok("Native Messaging installed"));
    } else {
        println!("{}", theme::warn("Native Messaging not installed"));
    }

    // 4, 5, 6. Extension connected & Heartbeat healthy (from daemon)
    let path = socket_path();
    if let Ok(mut stream) = UnixStream::connect(&path).await {
        let payload = wire::encode(&Request::Status).unwrap();
        let _ = stream.write_all(&payload).await;
        let mut len_buf = [0u8; 4];
        if stream.read_exact(&mut len_buf).await.is_ok() {
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut body = vec![0u8; len];
            if stream.read_exact(&mut body).await.is_ok() {
                if let Ok(Response::Status(status)) = wire::decode(&body) {
                    if status.extension_connected {
                        println!("{}", theme::ok("Extension connected"));
                        println!("{}", theme::ok("Heartbeat healthy"));
                    } else {
                        println!("{}", theme::warn("Extension connected: NO (Heartbeat missing)"));
                    }
                } else {
                    println!("{}", theme::warn("Extension connected: NO (Invalid response)"));
                }
            } else {
                println!("{}", theme::warn("Extension connected: NO (Read error)"));
            }
        } else {
            println!("{}", theme::warn("Extension connected: NO (Read error)"));
        }
    } else {
        println!("{}", theme::warn("Extension connected: NO (Daemon unreachable)"));
    }

    // 7. Browser detected
    let browsers = ["google-chrome", "chromium", "chromium-browser", "brave-browser", "microsoft-edge", "vivaldi", "opera"];
    let mut found_browser = false;
    for browser in browsers {
        if Command::new("which").arg(browser).output().is_ok_and(|o| o.status.success()) {
            found_browser = true;
            break;
        }
    }
    if found_browser {
        println!("{}", theme::ok("Browser detected"));
    } else {
        println!("{}", theme::warn("No supported Chromium browser detected"));
    }

    // 8. Manual reminders (as per MVP1 spec)
    println!("{}", theme::warn("Reminder: Ensure 'Allow in Incognito' is enabled in chrome://extensions"));
    println!("{}", theme::warn("Reminder: Load extension in all browser profiles you wish to protect"));

    println!("\n{}", theme::dim("Run `mindgate status` for more details."));
    Ok(())
}

async fn check_systemctl_active(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", unit])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn check_native_messaging() -> bool {
    let paths = [
        "/etc/opt/chrome/native-messaging-hosts/com.mindgate.protector.json",
        "/etc/chromium/native-messaging-hosts/com.mindgate.protector.json",
    ];
    for p in paths {
        if std::path::Path::new(p).exists() {
            return true;
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let user_path = std::path::Path::new(&home).join(".config/google-chrome/NativeMessagingHosts/com.mindgate.protector.json");
        if user_path.exists() {
            return true;
        }
    }
    false
}

async fn run_install() -> Result<()> {
    println!("{}", theme::dim("MindGate is typically installed via: curl -fsSL https://... | bash"));
    println!("{}", theme::dim("If you are running from a source checkout, run: sudo ./installer/install.sh"));
    Ok(())
}

async fn run_uninstall() -> Result<()> {
    println!("{}", theme::warn("This will remove MindGate completely. Continue? [y/N]"));
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).context("failed to read input")?;
    if !input.trim().eq_ignore_ascii_case("y") && !input.trim().eq_ignore_ascii_case("yes") {
        println!("Cancelled.");
        return Ok(());
    }

    let status = Command::new("sudo")
        .arg("/usr/local/bin/mindgate-uninstall.sh")
        .status()
        .context("failed to execute uninstall script")?;

    if !status.success() {
        anyhow::bail!("Uninstall failed. You may need to run `sudo ./installer/uninstall.sh` manually.");
    }
    Ok(())
}

async fn run_logs() -> Result<()> {
    let status = Command::new("journalctl")
        .args(["-u", "mindgated.service", "-f"])
        .status()
        .context("failed to execute journalctl")?;
    
    if !status.success() {
        anyhow::bail!("Failed to read logs. You may need to run this command with sudo.");
    }
    Ok(())
}

/// The Native Messaging Bridge Mode. Runs inside browser-spawned processes.
/// Translates between the browser's native-endian length-prefixed JSON
/// and the daemon's big-endian wire protocol.
async fn run_native_bridge() -> Result<()> {
    let path = socket_path();
    let socket = UnixStream::connect(&path)
        .await
        .with_context(|| format!("Bridge mode failed to connect to daemon socket at {}", path.display()))?;

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let (mut rx, mut tx) = socket.into_split();

    let client_to_daemon = async {
        let mut len_buf = [0u8; 4];
        loop {
            if stdin.read_exact(&mut len_buf).await.is_err() {
                break; // EOF or broken stdin
            }
            let len = u32::from_ne_bytes(len_buf) as usize;
            let mut body = vec![0u8; len];
            if stdin.read_exact(&mut body).await.is_err() {
                break;
            }

            // Parse request from browser, then convert to daemon's Big-Endian wire frame
            if let Ok(request) = serde_json::from_slice::<Request>(&body) {
                if let Ok(wire_payload) = wire::encode(&request) {
                    if tx.write_all(&wire_payload).await.is_err() {
                        break;
                    }
                }
            }
        }
        anyhow::Ok(())
    };

    let daemon_to_client = async {
        let mut len_buf = [0u8; 4];
        loop {
            if rx.read_exact(&mut len_buf).await.is_err() {
                break; // Socket closed by daemon
            }
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut body = vec![0u8; len];
            if rx.read_exact(&mut body).await.is_err() {
                break;
            }

            // Convert to browser's native-endian length prefix and pipe to stdout
            let native_len = (body.len() as u32).to_ne_bytes();
            if stdout.write_all(&native_len).await.is_err() {
                break;
            }
            if stdout.write_all(&body).await.is_err() {
                break;
            }
            let _ = stdout.flush().await;
        }
        anyhow::Ok(())
    };

    let _ = tokio::try_join!(client_to_daemon, daemon_to_client);
    Ok(())
}