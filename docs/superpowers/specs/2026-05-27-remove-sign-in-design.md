# Remove Sign-In Functionality from Zed

**Status:** Approved
**Date:** 2026-05-27
**Author:** Brainstorming session

## Goal

Remove all sign-in functionality from Zed, including the Zed account flow itself and every feature that requires it. Delete the now-dead code (crates, modules, settings, env vars). The result is a Zed fork that runs entirely without a Zed account.

## Non-goals

- Removing Copilot (has independent device-flow auth — kept).
- Removing BYO-API-key AI providers (Anthropic, OpenAI, Ollama, Supermaven, etc. — kept).
- Removing remote development / SSH (kept).
- Removing the extension marketplace (kept; no auth required to install extensions).
- Building any replacement for removed features (no settings-sync replacement, no self-hosted collab, etc.).

## Scope

### Removed

| Area | Detail |
|------|--------|
| Sign-in UI | `SignIn` action, title-bar account button, "Sign in to Zed" prompts (welcome, agent panel, edit-prediction button), `title_bar.show_sign_in` setting |
| Auth plumbing | zed.dev OAuth flow, `oauth_callback_server` crate, RSA token handshake in `rpc/auth.rs`, `ZED_IMPERSONATE` / `ZED_WEB_LOGIN` env vars, keychain credential storage |
| `client` crate | `UserStore`, `current_user()`, `Status`, RPC session, connection lifecycle. **HTTP client portion preserved** (extensions need it). |
| Collaboration | `collab`, `collab_ui`, `call`, `channel`, `livekit_client`, `livekit_api`, `livekit_server` crates. Channels panel, calls UI, shared projects, screen sharing, notifications panel (audit for non-collab usage before removal) |
| AI tied to Zed account | `language_models/src/provider/cloud.rs` (Zed cloud LLM), Zeta edit-prediction provider, plan/subscription code |
| Telemetry | `telemetry`, `telemetry_events` crates; all `telemetry::*` call sites |
| Auto-update | `auto_update`, `auto_update_ui` crates; "Check for Updates" action and notifications |
| Feedback | `feedback` crate, "Give Feedback" commands and menu entries |
| Settings sync | Account-tied settings sync mechanism (settings remain local-file driven) |

### Preserved

- Extension marketplace (anonymous downloads from zed.dev via `http_client`).
- Remote development (`remote`, `remote_server`, SSH).
- Copilot (`copilot`, `copilot_ui` — own auth).
- BYO-key AI providers (Anthropic, OpenAI, Ollama, Supermaven, etc.).
- Everything not auth-adjacent (editor, terminal, git, project panel, search, debugger, etc.).

## Execution strategy

Single working tree. Inside-out feature removal, leaves first, in 10 phases. Each phase keeps `cargo check --workspace` passing. **No commits until user approval at the end.**

1. **Telemetry** — remove `telemetry` + `telemetry_events` crates and all call sites.
2. **Auto-update** — remove `auto_update`, `auto_update_ui`, related actions/notifications.
3. **Feedback** — remove `feedback` crate, "Give Feedback" action and menus.
4. **Cloud LLM provider** — remove `language_models/src/provider/cloud.rs` and registration; keep other providers working.
5. **Zeta edit predictions** — remove the Zed-hosted edit-prediction path; keep Copilot/Supermaven.
6. **Settings sync** — remove account-tied sync mechanism. (If no separate sync subsystem exists and sync was always implicit in the cloud account path, this phase collapses into the `client` gut in phase 9 — confirmed during planning.)
7. **Collaboration** — remove `collab`, `collab_ui`, `call`, `channel`, `livekit_*` crates and consumers (collab panel registration, calls UI, shared-projects code, notifications panel).
8. **Sign-in UI** — remove `SignIn` action, title-bar sign-in button, sign-in dialogs and prompts.
9. **`client` crate gut** — remove `UserStore`, `current_user`, `Status`, RPC session, `oauth_callback_server`, `rpc/auth.rs`, keychain credentials, `ZED_IMPERSONATE` / `ZED_WEB_LOGIN`. Preserve HTTP plumbing for extensions (or migrate them to `http_client` directly — decided per actual coupling).
10. **Cleanup** — prune workspace members, dead deps, dead settings keys, keymap/menu entries that reference removed actions.

### Risks flagged for the planner

- **Phase 9 may need a 9a/9b split.** `current_user()` call sites are scattered. Likely: 9a replaces every call with its signed-out branch (deleting the gated branch), 9b deletes the type itself.
- **Extension HTTP coupling.** If extensions import `client::Client`, decide between threading `http_client::HttpClient` through or leaving `client` as a minimal re-export shim.
- **Keymap / menu drift.** Removed actions referenced from default keymaps/menus produce startup warnings — phase 10 must prune them.

## Verification

Run between phases:
- `cargo check --workspace` — must pass.

Run at natural breakpoints and once at the end:
- `./script/clippy` — must be clean.
- `cargo nextest run --workspace` — failures only in tests deleted with their parent crate; no surviving test regressions.

Run Zed at the end (manually-described checks since UI verification isn't scripted):
- No "Sign in" UI anywhere (title bar, command palette, welcome page, agent panel).
- No collab panel / channels / calls / shared projects / notifications panel.
- Cloud LLM provider gone; Anthropic/OpenAI/Ollama/Copilot/Supermaven still listed in agent settings.
- No "Check for Updates" / "Give Feedback" in command palette.
- Extensions panel lists and installs extensions.
- SSH remote dev still connects.

`cargo tree` shows no `livekit-*`, `collab`, `call`, `channel`, `auto_update`, `auto_update_ui`, `feedback`, `telemetry`, `telemetry_events`, `oauth_callback_server`.

## Definition of done

All of:
1. `cargo check --workspace` passes.
2. `./script/clippy` clean.
3. `cargo nextest run --workspace` — only deleted tests fail.
4. Manual Zed launch passes the checks in the Verification section.
5. Root `Cargo.toml` workspace members list has no removed crates.
6. **No commit made.** All changes uncommitted in working tree for user review.
