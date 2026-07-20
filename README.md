MINDGATE/
├── cli/
│   ├── src/
│   │   └── main.rs
│   └── Cargo.toml
├── common/
│   ├── src/
│   │   └── lib.rs
│   └── Cargo.toml
├── daemon/
│   ├── src/
│   │   ├── engine.rs
│   │   ├── guardian.rs
│   │   ├── lock.rs
│   │   ├── main.rs
│   │   ├── self_watch.rs
│   │   ├── server.rs
│   │   └── store.rs
│   └── Cargo.toml
├── extension/
│   ├── icons/
│   ├── background.js
│   ├── block.html
│   ├── block.js
│   ├── com.mindgate.protector.json
│   ├── content.js
│   ├── logo.png
│   ├── manifest.json
│   ├── mindgate.sh
│   └── quotes.json
├── installer/
│   ├── systemd/
│   │   ├── mindgate-watchdog.service
│   │   └── mindgated.service
│   ├── install.sh
│   ├── mindgate-watchdog.sh
│   └── uninstall.sh
├── target/
│   ├── debug/
│   ├── release/
│   ├── .rustc_info.json
│   └── CACHEDIR.TAG
├── .gitignore
├── Cargo.lock
├── Cargo.toml
├── CONTEXT.md
├── LICENSE
├── mindgate-env.sh
└── README.md


COMMANDS:

# Start/stop/restart
mindgate start
mindgate stop
mindgate restart

# Check health
mindgate doctor
mindgate status
mindgate logs

# Install/uninstall
mindgate install
mindgate uninstall

# Help
mindgate --help