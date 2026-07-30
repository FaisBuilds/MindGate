# Adding Features & Fixes to MindGate — The `adapters/` Rule

MindGate's existing core is stable and battle-tested.

The core should be treated as **frozen by default**. New features, integrations, platform support, and bug fixes should be implemented through `adapters/` whenever possible, without modifying existing core implementation.

The architecture follows the **Open/Closed Principle** and a **Ports and Adapters** approach:

* Existing core behavior remains stable.
* New functionality is added around the core.
* Adapters interact with the core through narrow, explicit interfaces.
* Existing core logic is not silently modified to accommodate new work.

The goal is simple:

> **Extend MindGate without destabilizing the parts that already work.**

---

## The Core Rule

**Do not modify existing core implementation when working on a new feature, integration, platform, or bug fix unless the user explicitly approves a core change.**

This applies even when:

* The required core change is only one or two lines.
* The core change seems harmless.
* The core change would make the adapter easier to implement.
* The core change would be cleaner or more elegant.
* The adapter appears to require a small change to existing logic.
* You believe you have discovered a bug inside the core.

Do not silently edit the core as part of an adapter task.

If the requested work cannot be correctly implemented without modifying the core:

1. Stop before modifying the core.
2. Explain why an adapter-based solution is insufficient.
3. Identify the exact core component that needs to change.
4. Describe the smallest core change that would be required.
5. Wait for explicit approval before modifying the core.

A core modification must never be made implicitly as part of an adapter task.

---

# Everything New Lives in `adapters/`

The repository follows this structure:

```text
MindGate/
├── adapters/
│   ├── system-lock-resume/
│   │   └── .gitkeep
│   │
│   ├── firefox-platform/
│   │   └── ...
│   │
│   └── <next-adapter>/
│       └── ...
│
├── daemon/                  # Core — frozen by default
├── extension/               # Core — frozen by default
├── installer/
├── ADAPTERS.md
└── ...
```

Every new feature, integration, platform, or bug fix should first be considered for implementation inside:

```text
adapters/<name>/
```

Examples:

```text
adapters/firefox-platform/
adapters/lock-screen/
adapters/system-lock-resume/
adapters/<future-feature>/
```

An adapter should be independently removable whenever technically possible. Removing an adapter should leave the existing core behavior unchanged.

---

# Rust Adapters

Anything that runs inside the daemon should be isolated as its own Rust crate.

Each Rust adapter:

* Has its own `Cargo.toml`.
* Is listed as a member of the root Cargo workspace.
* Is added as a path dependency only where required.
* Exposes the smallest possible public interface.
* Does not access private core state.
* Does not duplicate or reimplement core logic unnecessarily.
* Is disabled by default unless explicitly enabled.
* Fails safe: errors, timeouts, or uncertainty must never cause the adapter to take an unsafe action.

The adapter should interact with the core through a narrow, explicit interface.

If an adapter requires a new interface from the core, do not automatically modify the core to create that interface.

Instead, stop and report:

* Why the existing interface is insufficient.
* What interface is needed.
* What minimal core change would be required.

Wait for explicit approval before modifying the core.

---

# Non-Rust Adapters

New browser or platform targets should live entirely under:

```text
adapters/<name>-platform/
```

Examples:

```text
adapters/firefox-platform/
adapters/<future-browser>-platform/
```

Do not modify the existing Chromium extension merely to support another browser or platform.

Keep new platform implementations separate from existing implementations until there is a demonstrated reason to share code.

Do not prematurely extract shared logic into the existing extension or core. Duplication between two implementations is preferable to introducing unnecessary coupling before shared behavior is proven.

---

# Bug Fixes

Bug fixes follow the same rule as new features.

First determine where the bug can be fixed without modifying the existing core.

For example:

```text
User: "Fix the lock-screen bug."
```

The expected approach is:

```text
adapters/lock-screen/
```

Investigate and fix the issue there without changing the established core behavior.

If the investigation reveals that the bug is genuinely caused by existing core logic and cannot be correctly fixed from the adapter:

* Do not silently patch the core.
* Do not make a "small" core change as part of the fix.
* Report the core issue separately.
* Explain the minimum required core change.
* Wait for explicit approval.

A bug being discovered in the core does not automatically authorize modifying the core.

---

# The Test Before Editing Core

Before modifying any existing core file, ask:

> **"Was I explicitly told to modify the core?"**

If the answer is **NO**:

* Do not modify it.
* Do not refactor it.
* Do not clean it up.
* Do not "improve" it.
* Do not make a small change to make an adapter easier.
* Find an adapter-based solution instead.
* If no correct adapter-based solution exists, stop and explain the blocker.

If the answer is **YES**:

* Make the smallest possible core change.
* Do not refactor unrelated code.
* Do not clean up unrelated code.
* Do not change unrelated behavior.
* Clearly identify what core code was changed and why.

---

# What Counts as a Core Change?

A core change includes, but is not limited to:

* Editing existing logic in `daemon/`.
* Editing existing blocking logic in `extension/background.js`.
* Editing existing blocking logic in `extension/content.js`.
* Changing established core decision-making behavior.
* Refactoring core code to accommodate an adapter.
* Moving existing core logic to make an adapter easier to implement.
* Adding "just one line" to core code without explicit approval.

The size of the change does not matter.

A one-line unauthorized core modification is still a core modification.

---

# What Counts as an Adapter?

An adapter is a self-contained addition that extends MindGate without changing the established behavior of the core.

Examples include:

* Firefox browser support.
* New browser or platform integrations.
* Optional presence detection.
* Lock-screen functionality.
* New external integrations.
* New optional capabilities.
* Platform-specific implementations.

The adapter should plug into MindGate through the narrowest practical interface and remain isolated from unrelated core implementation.

---

# Final Principle

MindGate's core is the **stable foundation**.

Adapters are the **extension layer**.

The default direction is:

```text
                ┌───────────────────────┐
                │      MindGate Core    │
                │                       │
                │   Stable Behaviour   │
                └───────────▲───────────┘
                            │
                    Explicit Interface
                            │
              ┌─────────────┴─────────────┐
              │                           │
      ┌───────┴────────┐         ┌────────┴───────┐
      │ Firefox Adapter│         │ Lock Screen    │
      │                │         │ Adapter        │
      └────────────────┘         └────────────────┘
```

**Prefer adding adapters over modifying the core.**

**Never modify the core implicitly.**

**If an adapter genuinely cannot solve the problem, stop and ask for approval before touching the core.**
