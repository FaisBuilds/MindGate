use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mindgate_common::{socket_path, wire, Request, Response, StatusInfo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

#[derive(Parser, Debug)]
#[command(name = "mindgate", about = "Modern, open-source Linux focus tool", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Block a website domain-wide (network layer)
    Add {
        /// The domain to block (e.g., reddit.com)
        domain: String,
    },
    /// Unblock a website domain-wide
    Remove {
        /// The domain to unblock
        domain: String,
    },
    /// Block a specific subreddit (browser layer)
    AddSubreddit {
        /// The subreddit name to block (e.g., gonewild)
        name: String,
    },
    /// Unblock a specific subreddit
    RemoveSubreddit {
        /// The subreddit name to unblock
        name: String,
    },
    /// Block any URL containing a keyword (browser layer)
    AddKeyword {
        /// The keyword value to block
        value: String,
    },
    /// Unblock a keyword
    RemoveKeyword {
        /// The keyword value to unblock
        value: String,
    },
    /// List all currently configured rules
    List,
    /// View detailed daemon engine status and ruleset metrics
    Status,
    /// Hidden subcommand used by browser extensions to bridge stdio to the daemon socket
    #[command(hide = true)]
    __NativeBridge,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::__NativeBridge => {
            run_native_bridge().await?;
        }
        cmd => {
            let req = match cmd {
                Commands::Add { domain } => Request::AddWebsite { domain },
                Commands::Remove { domain } => Request::RemoveWebsite { domain },
                Commands::AddSubreddit { name } => Request::AddSubreddit { subreddit: name },
                Commands::RemoveSubreddit { name } => Request::RemoveSubreddit { subreddit: name },
                Commands::AddKeyword { value } => Request::AddKeyword { value },
                Commands::RemoveKeyword { value } => Request::RemoveKeyword { value },
                Commands::List => Request::List,
                Commands::Status => Request::Status,
                _ => unreachable!(),
            };

            if let Err(e) = send_request_and_print(req).await {
                eprintln!("Error: {e:#}");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

/// Connects to the daemon's Unix socket, transmits the request payload,
/// and processes the response into a readable visual CLI format.
async fn send_request_and_print(req: Request) -> Result<()> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .await
        .with_context(|| format!("Could not connect to daemon at {}", path.display()))?;

    // Send the length-prefixed request
    let payload = wire::encode(&req)?;
    stream.write_all(&payload).await?;

    // Read the 4-byte length prefix of response
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    // Read response body
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;

    let response: Response = wire::decode(&body)?;
    match response {
        Response::Ok => {
            println!("✓ Operation succeeded.");
        }
        Response::Error { message } => {
            anyhow::bail!("{message}");
        }
        Response::Rules(rules) => {
            println!("--- MindGate Rule Set ---");
            println!("\n[Websites (Network Layer)]");
            if rules.websites.is_empty() {
                println!("  (None)");
            } else {
                for w in rules.websites {
                    println!("  * {}", w.domain);
                }
            }

            println!("\n[Subreddits (Browser Layer)]");
            if rules.subreddits.is_empty() {
                println!("  (None)");
            } else {
                for s in rules.subreddits {
                    println!("  * r/{}", s.subreddit);
                }
            }

            println!("\n[Keywords (Browser Layer)]");
            if rules.keywords.is_empty() {
                println!("  (None)");
            } else {
                for k in rules.keywords {
                    println!("  * {}", k.value);
                }
            }
        }
        Response::Status(status) => {
            print_status_info(status);
        }
    }

    Ok(())
}

fn print_status_info(status: StatusInfo) {
    println!("--- MindGate Daemon Status ---");
    println!("Daemon Running:       {}", if status.daemon_running { "YES" } else { "NO" });
    println!("nftables Active:      {}", if status.nft_table_active { "YES" } else { "NO (Dry-run mode active)" });
    println!("Extension Connected:  {}", if status.extension_connected { "YES" } else { "NO (Check browser background)" });
    println!("Active Locks:         {}", if status.lock.locked { "YES" } else { "NO" });
    println!("\n[Metrics]");
    println!("Total Rules:          {}", status.rule_count);
    println!("  - Websites:         {}", status.website_count);
    println!("  - Subreddits:       {}", status.subreddit_count);
    println!("  - Keywords:         {}", status.keyword_count);
}

/// The Native Messaging Bridge Mode. Runs inside browser-spawned processes.
/// Directly bridges standard input/output streams to the daemon's Unix socket
/// using length-prefixed protocol forwarding.
async fn run_native_bridge() -> Result<()> {
    let path = socket_path();
    let socket = UnixStream::connect(&path)
    .await
        .with_context(|| format!("Bridge mode failed to connect to daemon socket at {}", path.display()))?;

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    // Use a split-task approach to concurrently route stdin -> socket and socket -> stdout
    let (mut rx, mut tx) = socket.into_split();

    let client_to_daemon = async {
        let mut len_buf = [0u8; 4];
        loop {
            // Read length prefix from Browser via stdin
            if stdin.read_exact(&mut len_buf).await.is_err() {
                break; // EOF or broken stdin (Browser closed tab/extension crashed)
            }
            let len = u32::from_ne_bytes(len_buf) as usize; // WebExtensions send standard native endianness
            
            let mut body = vec![0u8; len];
            if stdin.read_exact(&mut body).await.is_err() {
                break;
            }

            // Parse request to make sure it's valid, then convert native-endian to daemon's Big-Endian wire frame
            let req: Result<Request, _> = serde_json::from_slice(&body);
            if let Ok(request) = req {
                let wire_payload = match wire::encode(&request) {
                    Ok(p) => p,
                    Err(_) => break,
                };
                if tx.write_all(&wire_payload).await.is_err() {
                    break;
                }
            }
        }
        anyhow::Ok(())
    };

    let daemon_to_client = async {
        let mut len_buf = [0u8; 4];
        loop {
            // Read Big-Endian length from the daemon's Unix socket
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

    // Keep active until either standard input or socket gets closed
    let _ = tokio::try_join!(client_to_daemon, daemon_to_client);

    Ok(())
}