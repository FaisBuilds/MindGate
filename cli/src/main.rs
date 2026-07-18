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
    /// Block something. Three forms:
    ///   mindgate add <domain>                 — block a whole site (network layer)
    ///   mindgate add path <domain>/<path>     — block a path prefix on a site (browser layer)
    ///   mindgate add keyword <value>          — block any URL/page containing a keyword (browser layer)
    ///
    /// There is no site-specific command (no `add-subreddit`, etc), and
    /// no hyphenated commands (no `add-keyword`, etc) — "path" and
    /// "keyword" are just the second word after `add`/`remove`.
    ///
    /// Examples:
    ///   mindgate add youtube.com
    ///   mindgate add path reddit.com/r/gaming
    ///   mindgate add keyword nsfw
    Add {
        /// Either `<domain>`, or the literal `path` followed by
        /// `<domain>/<path-prefix>`, or the literal `keyword` followed
        /// by a value (e.g. `path reddit.com/r/gaming`, `keyword nsfw`)
        #[arg(num_args = 1..=2)]
        target: Vec<String>,
    },
    /// Unblock something. Same forms as `add`:
    ///   mindgate remove <domain>
    ///   mindgate remove path <domain>/<path>
    ///   mindgate remove keyword <value>
    Remove {
        /// Either `<domain>`, or the literal `path` followed by
        /// `<domain>/<path-prefix>`, or the literal `keyword` followed
        /// by a value
        #[arg(num_args = 1..=2)]
        target: Vec<String>,
    },
    /// List all currently configured rules
    List,
    /// View detailed daemon engine status and ruleset metrics
    Status,
    /// Commit the current ruleset and activate enforcement. THIS
    /// CANNOT BE UNDONE EARLY — there is no unlock command. The
    /// ruleset stays frozen until the duration elapses.
    ///
    /// DURATION examples: 5min, 4h, 1d, 2w, 6mo, 1y, or `forever`.
    ///   min = minutes (1-1440)      — mainly for dev/testing
    ///   h   = hours   (1-168)
    ///   d   = days    (1-365)
    ///   w   = weeks   (1-52)
    ///   mo  = months  (1-12)
    ///   y   = years   (1-10)
    Lock {
        /// e.g. "5min", "4h", "1d", "2w", "6mo", "1y", or "forever"
        duration: String,
    },
    /// Hidden subcommand used by browser extensions to bridge stdio to
    /// the daemon socket. Explicitly named (rather than left to clap's
    /// default kebab-case rendering of `NativeBridge`) so it's
    /// "nativebridge", not "native-bridge" — hidden from --help either
    /// way, but keeps the CLI's surface hyphen-free even for the one
    /// subcommand a human never types themselves.
    #[command(name = "nativebridge", hide = true)]
    NativeBridge,
}

/// Parses a duration string like "5min", "4h", "1d", "2w", "6mo",
/// "1y", or the literal "forever" into seconds. `None` return value
/// means "forever" (no timer — matches `Request::Lock`'s
/// `duration_secs: Option<u64>`, where `None` already means untimed).
/// Every unit has an explicit range; out-of-range or unrecognized
/// input is rejected here, client-side, before it ever reaches the
/// daemon — there's no reason to round-trip a socket call for input
/// that's already obviously invalid.
///
/// NOTE: months is `mo`, not `m` — `m` was ambiguous with minutes.
/// If you have scripts using the old `6m` = "6 months" form, update
/// them to `6mo`.
fn parse_duration(input: &str) -> Result<Option<u64>, String> {
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("forever") {
        return Ok(None);
    }

    let (num_str, unit) = trimmed.split_at(
        trimmed
            .find(|c: char| !c.is_ascii_digit())
            .ok_or_else(|| format!("'{trimmed}' is missing a unit (e.g. 5min, 4h, 1d, 2w, 6mo, 1y)"))?,
    );

    let n: u64 = num_str
        .parse()
        .map_err(|_| format!("'{num_str}' isn't a valid number"))?;

    if n == 0 {
        return Err("duration must be at least 1".to_string());
    }

    const MINUTE: u64 = 60;
    const HOUR: u64 = 3600;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    const MONTH: u64 = 30 * DAY;
    const YEAR: u64 = 365 * DAY;

    let (seconds, max, unit_name) = match unit {
        "min" => (n * MINUTE, 1440, "minutes"),
        "h" => (n * HOUR, 168, "hours"),
        "d" => (n * DAY, 365, "days"),
        "w" => (n * WEEK, 52, "weeks"),
        "mo" => (n * MONTH, 12, "months"),
        "y" => (n * YEAR, 10, "years"),
        other => {
            return Err(format!(
                "unrecognized unit '{other}' — use min, h, d, w, mo, y, or the word 'forever'"
            ))
        }
    };

    if n > max {
        return Err(format!("{n}{unit} is out of range — max is {max} {unit_name}"));
    }

    Ok(Some(seconds))
}

/// What `add`/`remove`'s trailing args resolved to.
enum Target {
    /// `mindgate add youtube.com` — whole-domain, network-layer block.
    Website(String),
    /// `mindgate add path reddit.com/r/gaming` — domain-scoped
    /// path-prefix, browser-layer block.
    Path { domain: String, path: String },
    /// `mindgate add keyword nsfw` — URL/page-content keyword,
    /// browser-layer block.
    Keyword(String),
}

/// Parses the `Vec<String>` collected by clap for `add`/`remove` into
/// a `Target`. Three shapes are accepted:
///   [domain]                 -> Target::Website
///   ["path", combined]       -> Target::Path (via split_domain_path)
///   ["keyword", value]       -> Target::Keyword
/// Anything else (0 args, a bare "path"/"keyword" with nothing after
/// it, 3+ args) is a usage error, reported client-side before any
/// socket round trip.
fn parse_target(args: &[String]) -> Result<Target, String> {
    match args {
        [] => Err(
            "expected a domain, `path <domain>/<path>`, or `keyword <value>` — e.g. \
             `mindgate add youtube.com`, `mindgate add path reddit.com/r/gaming`, or \
             `mindgate add keyword nsfw`"
                .to_string(),
        ),
        [only] => {
            if only.eq_ignore_ascii_case("path") {
                Err(
                    "`path` needs a `<domain>/<path>` argument, e.g. \
                     `mindgate add path reddit.com/r/gaming`"
                        .to_string(),
                )
            } else if only.eq_ignore_ascii_case("keyword") {
                Err(
                    "`keyword` needs a value, e.g. `mindgate add keyword nsfw`".to_string(),
                )
            } else {
                Ok(Target::Website(only.trim().to_lowercase()))
            }
        }
        [literal, combined] if literal.eq_ignore_ascii_case("path") => {
            let (domain, path) = split_domain_path(combined)?;
            Ok(Target::Path { domain, path })
        }
        [literal, value] if literal.eq_ignore_ascii_case("keyword") => {
            Ok(Target::Keyword(value.clone()))
        }
        _ => Err(format!(
            "unrecognized arguments '{}' — usage: `mindgate add <domain>`, \
             `mindgate add path <domain>/<path>`, or `mindgate add keyword <value>`",
            args.join(" ")
        )),
    }
}

/// Splits a combined `domain/path` string like `reddit.com/r/gaming`
/// into `("reddit.com", "/r/gaming")`. The path keeps its leading `/`
/// (matches the shape `PathRule.path` and `content.js`'s prefix check
/// already expect). Domain is lowercased for consistent dedup against
/// whatever's already staged; the path's case is left alone since some
/// sites' paths are case-sensitive.
fn split_domain_path(combined: &str) -> Result<(String, String), String> {
    let trimmed = combined.trim();
    let slash_idx = trimmed.find('/').ok_or_else(|| {
        format!(
            "'{trimmed}' needs a path — e.g. `mindgate add path {trimmed}/r/gaming`"
        )
    })?;

    let domain = &trimmed[..slash_idx];
    let path = &trimmed[slash_idx..]; // keeps the leading '/'

    if domain.is_empty() {
        return Err(format!("'{trimmed}' is missing a domain before the path"));
    }
    if path.len() <= 1 {
        return Err(format!(
            "'{trimmed}' is missing a path after the domain — e.g. \
             `mindgate add path {domain}/r/gaming`"
        ));
    }

    Ok((domain.to_lowercase(), path.to_string()))
}

#[cfg(test)]
mod target_parsing_tests {
    use super::*;

    #[test]
    fn bare_domain_is_a_website_target() {
        match parse_target(&["youtube.com".to_string()]).unwrap() {
            Target::Website(d) => assert_eq!(d, "youtube.com"),
            _ => panic!("expected Website"),
        }
    }

    #[test]
    fn domain_is_lowercased() {
        match parse_target(&["YouTube.com".to_string()]).unwrap() {
            Target::Website(d) => assert_eq!(d, "youtube.com"),
            _ => panic!("expected Website"),
        }
    }

    #[test]
    fn path_literal_splits_domain_and_path() {
        match parse_target(&["path".to_string(), "reddit.com/r/gaming".to_string()]).unwrap() {
            Target::Path { domain, path } => {
                assert_eq!(domain, "reddit.com");
                assert_eq!(path, "/r/gaming");
            }
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn path_literal_is_case_insensitive() {
        assert!(parse_target(&["PATH".to_string(), "x.com/foo".to_string()]).is_ok());
    }

    #[test]
    fn path_preserves_nested_prefix() {
        match parse_target(&["path".to_string(), "youtube.com/shorts".to_string()]).unwrap() {
            Target::Path { domain, path } => {
                assert_eq!(domain, "youtube.com");
                assert_eq!(path, "/shorts");
            }
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn keyword_literal_captures_value() {
        match parse_target(&["keyword".to_string(), "nsfw".to_string()]).unwrap() {
            Target::Keyword(value) => assert_eq!(value, "nsfw"),
            _ => panic!("expected Keyword"),
        }
    }

    #[test]
    fn keyword_literal_is_case_insensitive() {
        assert!(parse_target(&["KEYWORD".to_string(), "nsfw".to_string()]).is_ok());
    }

    #[test]
    fn bare_keyword_literal_with_no_value_is_rejected() {
        assert!(parse_target(&["keyword".to_string()]).is_err());
    }

    #[test]
    fn path_without_domain_slash_path_is_rejected() {
        // "path reddit.com" alone has no '/' to split on.
        assert!(parse_target(&["path".to_string(), "reddit.com".to_string()]).is_err());
    }

    #[test]
    fn bare_path_literal_with_no_second_arg_is_rejected() {
        assert!(parse_target(&["path".to_string()]).is_err());
    }

    #[test]
    fn no_args_is_rejected() {
        assert!(parse_target(&[]).is_err());
    }

    #[test]
    fn too_many_args_is_rejected() {
        assert!(parse_target(&[
            "path".to_string(),
            "reddit.com/r/gaming".to_string(),
            "extra".to_string()
        ])
        .is_err());
    }

    #[test]
    fn domain_only_named_path_is_rejected_without_second_arg() {
        // Guards against a user typing just `mindgate add path` and
        // getting a confusing "blocked domain: path" instead of a
        // clear usage error.
        assert!(parse_target(&["path".to_string()]).is_err());
    }
}

fn format_duration_human(input: &str, seconds: Option<u64>) -> String {
    match seconds {
        None => "forever".to_string(),
        Some(_) => input.to_string(),
    }
}

#[cfg(test)]
mod duration_tests {
    use super::*;

    #[test]
    fn parses_minutes() {
        assert_eq!(parse_duration("5min"), Ok(Some(300)));
    }

    #[test]
    fn parses_hours() {
        assert_eq!(parse_duration("4h"), Ok(Some(4 * 3600)));
    }

    #[test]
    fn parses_months_as_mo_not_m() {
        assert_eq!(parse_duration("6mo"), Ok(Some(6 * 30 * 86400)));
    }

    #[test]
    fn old_bare_m_unit_is_now_rejected() {
        // Regression guard: `m` used to mean months, which collided
        // with the intuitive reading of `m` as minutes. It must now
        // be rejected outright rather than silently reinterpreted.
        assert!(parse_duration("6m").is_err());
    }

    #[test]
    fn seconds_unit_is_not_supported() {
        // Explicitly not supported — minutes is the shortest unit.
        assert!(parse_duration("30s").is_err());
    }

    #[test]
    fn parses_forever() {
        assert_eq!(parse_duration("forever"), Ok(None));
        assert_eq!(parse_duration("FOREVER"), Ok(None));
    }

    #[test]
    fn rejects_zero() {
        assert!(parse_duration("0min").is_err());
    }

    #[test]
    fn rejects_out_of_range() {
        assert!(parse_duration("2000min").is_err()); // max is 1440
        assert!(parse_duration("999h").is_err()); // max is 168
    }

    #[test]
    fn rejects_unknown_unit() {
        assert!(parse_duration("5x").is_err());
    }

    #[test]
    fn rejects_missing_unit() {
        assert!(parse_duration("5").is_err());
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::NativeBridge => {
            run_native_bridge().await?;
        }
        Commands::Lock { duration } => {
            let duration_secs = match parse_duration(&duration) {
                Ok(d) => d,
                Err(msg) => {
                    eprintln!("Error: {msg}");
                    std::process::exit(1);
                }
            };

            if duration_secs.is_none() {
                // `forever` has no timer and no unlock command — this
                // is the one place in the whole CLI where we slow the
                // user down on purpose rather than just doing the
                // thing. Three separate, distinctly-worded
                // confirmations, not one dialog with a checkbox: each
                // one has to be actively re-read and re-typed, which
                // is the point — friction here is intentional, matching
                // MindGate's own stated philosophy, not an oversight.
                if !confirm(
                    "⚠️  You are about to lock your ruleset FOREVER. \
                     There is no unlock command. Continue? [y/N] ",
                )? {
                    println!("Cancelled.");
                    return Ok(());
                }
                if !confirm(
                    "⚠️  This is permanent. The only way out is deleting \
                     and reinstalling MindGate. Are you sure? [y/N] ",
                )? {
                    println!("Cancelled.");
                    return Ok(());
                }
                if !confirm(
                    "⚠️  Last chance. Type y to lock this ruleset forever, \
                     with no way back: [y/N] ",
                )? {
                    println!("Cancelled.");
                    return Ok(());
                }
            }

            let req = Request::Lock { duration_secs };
            match send_request_and_print_lock(req, &duration, duration_secs).await {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("Error: {e:#}");
                    std::process::exit(1);
                }
            }
        }
        cmd => {
            let req = match cmd {
                Commands::Add { target } => match parse_target(&target) {
                    Ok(Target::Website(domain)) => Request::AddWebsite { domain },
                    Ok(Target::Path { domain, path }) => Request::AddPath { domain, path },
                    Ok(Target::Keyword(value)) => Request::AddKeyword { value },
                    Err(msg) => {
                        eprintln!("Error: {msg}");
                        std::process::exit(1);
                    }
                },
                Commands::Remove { target } => match parse_target(&target) {
                    Ok(Target::Website(domain)) => Request::RemoveWebsite { domain },
                    Ok(Target::Path { domain, path }) => Request::RemovePath { domain, path },
                    Ok(Target::Keyword(value)) => Request::RemoveKeyword { value },
                    Err(msg) => {
                        eprintln!("Error: {msg}");
                        std::process::exit(1);
                    }
                },
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

/// Prompts `prompt` and reads a line from stdin, returning true only
/// for an explicit "y" or "yes" (case-insensitive). Anything else —
/// including just pressing Enter — is a no, matching the `[y/N]`
/// convention shown in the prompt text.
fn confirm(prompt: &str) -> Result<bool> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let answer = line.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

/// Same wire round-trip as `send_request_and_print`, but with a
/// success message specific to locking, since "✓ Operation succeeded."
/// doesn't convey what actually just happened (enforcement activating,
/// permanently or for a set duration) the way it should for the one
/// command in this CLI that can't be undone.
async fn send_request_and_print_lock(
    req: Request,
    duration_input: &str,
    duration_secs: Option<u64>,
) -> Result<()> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .await
        .with_context(|| format!("Could not connect to daemon at {}", path.display()))?;

    let payload = wire::encode(&req)?;
    stream.write_all(&payload).await?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;

    let response: Response = wire::decode(&body)?;
    match response {
        Response::Ok => {
            println!(
                "🔒 Locked — {}. Enforcement is now active. This cannot be undone early.",
                format_duration_human(duration_input, duration_secs)
            );
        }
        Response::Error { message } => anyhow::bail!("{message}"),
        other => {
            // Lock only ever responds Ok/Error server-side, but handle
            // the rest rather than silently dropping them if that
            // changes later.
            println!("{other:?}");
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

            println!("\n[Keywords (Browser Layer)]");
            if rules.keywords.is_empty() {
                println!("  (None)");
            } else {
                for k in rules.keywords {
                    println!("  * {}", k.value);
                }
            }

            // No Reddit-specific section here by design — `paths` is
            // the one general mechanism (domain + path prefix), and
            // covers reddit.com/r/gaming exactly the same way it
            // covers youtube.com/shorts. Any LEGACY `rules.subreddits`
            // entries (from before this command existed) are folded
            // in here too, displayed as the equivalent reddit.com/r/
            // path, so nothing from an older rules.toml goes missing.
            println!("\n[Paths (Browser Layer)]");
            if rules.paths.is_empty() && rules.subreddits.is_empty() {
                println!("  (None)");
            } else {
                for p in &rules.paths {
                    println!("  * {}{}", p.domain, p.path);
                }
                for s in &rules.subreddits {
                    println!("  * reddit.com/r/{} (legacy)", s.subreddit);
                }
            }
        }
        Response::Status(status) => {
            print_status_info(status);
        }
    }

    Ok(())
}

fn format_seconds_human(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn print_status_info(status: StatusInfo) {
    println!("--- MindGate Daemon Status ---");
    println!("Daemon Running:       {}", if status.daemon_running { "YES" } else { "NO" });
    println!("nftables Active:      {}", if status.nft_table_active { "YES" } else { "NO (Dry-run mode active)" });
    println!("Extension Connected:  {}", if status.extension_connected { "YES" } else { "NO (Check browser background)" });
    let lock_detail = if !status.lock.locked {
        "NO".to_string()
    } else {
        match status.lock.unlock_at {
            Some(unlock_at) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let remaining = unlock_at.saturating_sub(now);
                format!("YES ({} remaining)", format_seconds_human(remaining))
            }
            None => "YES (forever — no unlock)".to_string(),
        }
    };
    println!("Active Locks:         {lock_detail}");
    println!("\n[Metrics]");
    println!("Total Rules:          {}", status.rule_count);
    println!("  - Websites:         {}", status.website_count);
    // Combined: `path_count` (the current mechanism) plus
    // `subreddit_count` (legacy entries from before `add path`
    // existed) — both are enforced the same way, so they're shown as
    // one number rather than a Reddit-specific line item.
    println!("  - Paths:            {}", status.path_count + status.subreddit_count);
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