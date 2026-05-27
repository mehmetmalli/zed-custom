# Remove Sign-In from Zed — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove all sign-in functionality and every feature gated on a Zed account, deleting the now-dead code. Result is a Zed fork that runs entirely without a Zed account.

**Architecture:** Inside-out feature removal in 10 phases (= 10 tasks). Each phase removes one feature area plus its now-dead code, keeping `cargo check --workspace` passing between phases. Single working tree. **NO COMMITS at any point** — the user wants one final commit they make themselves after review.

**Tech Stack:** Rust workspace (gpui-based desktop app). Verification via `cargo check`, `./script/clippy`, `cargo nextest`.

**Spec:** `docs/superpowers/specs/2026-05-27-remove-sign-in-design.md`

---

## Ground rules for every task

1. **Never run `git commit`, `git add`, or any state-changing git command.** All edits stay uncommitted.
2. **Use `cargo check --workspace` as the per-task gate.** Do not move to the next task if it fails.
3. **When removing call sites in many files, use Grep to enumerate, then edit each.** Don't blanket-delete with `sed`.
4. **When deleting a crate:**
   - Delete the crate directory: `rm -rf crates/<name>`
   - Remove its entry from `members = [...]` in `/Users/memo/Desktop/zed/Cargo.toml` (lines 3–257).
   - Remove its entry from the `[workspace.dependencies]` section in the same file.
   - Remove `<name> = { workspace = true }` from every other `Cargo.toml` that depended on it (use Grep to find them).
5. **Removed actions referenced from keymaps cause startup warnings.** When you remove an action, also remove it from `assets/keymaps/default-macos.json`, `default-linux.json`, `default-windows.json`, and `crates/zed/src/zed/app_menus.rs`.
6. **Keep `http_client` working** — extensions and the marketplace use it. Don't touch it unless cleaning up unused features.

---

## Task 1: Remove telemetry

**Files:**
- Delete: `crates/telemetry/`
- Delete: `crates/telemetry_events/`
- Modify: `/Users/memo/Desktop/zed/Cargo.toml` (remove members + workspace deps entries)
- Modify: every `Cargo.toml` that depends on `telemetry` or `telemetry_events`
- Modify: every `.rs` file that imports or calls `telemetry::*` or `telemetry_events::*`

- [ ] **Step 1.1: Enumerate dependents**

Run:
```
Grep pattern: "telemetry(_events)?\\s*=\\s*\\{\\s*workspace" path: . glob: "**/Cargo.toml"
Grep pattern: "use telemetry(_events)?(::|;)" path: . glob: "**/*.rs"
Grep pattern: "telemetry::(report_event|report_app_event|event!|flush_events)" path: . glob: "**/*.rs"
```

Record the file list. Expect dependents in `crates/client`, `crates/editor`, `crates/zed`, possibly `crates/copilot`, `crates/extension_host`, others.

- [ ] **Step 1.2: Remove every telemetry call site**

For each file from step 1.1:
- Delete the `use telemetry::*` / `use telemetry_events::*` lines.
- Delete the call expression. If the call returned a value used downstream, replace with the appropriate no-op (`()` for `;`-terminated calls; otherwise inline the side-effect-free path).
- Delete any helper functions whose only purpose was to emit telemetry.
- Delete struct fields whose only purpose was a `Telemetry` handle or `TelemetryEventStream`.

If a struct is constructed with a `telemetry: Arc<Telemetry>` field that callers passed in, also remove the field from constructors and call sites — this can cascade. Follow the cascade until `cargo check` is clean.

- [ ] **Step 1.3: Remove `telemetry` and `telemetry_events` from `Cargo.toml` files**

For each `Cargo.toml` in the list:
- Remove the `telemetry = { workspace = true }` / `telemetry_events = { workspace = true }` lines (also `optional = true` variants).
- Remove these from any `[features]` lists that enable them.

Then in `/Users/memo/Desktop/zed/Cargo.toml`:
- Remove the `"crates/telemetry"` and `"crates/telemetry_events"` lines from `members = [...]`.
- Remove the `telemetry = { path = ... }` and `telemetry_events = { path = ... }` lines from `[workspace.dependencies]`.

- [ ] **Step 1.4: Delete the crate directories**

```
rm -rf /Users/memo/Desktop/zed/crates/telemetry
rm -rf /Users/memo/Desktop/zed/crates/telemetry_events
```

- [ ] **Step 1.5: Verify**

```
cargo check --workspace
```

Expected: success. If failures remain, they are unmigrated call sites or stray `Cargo.toml` references — fix them and re-run.

- [ ] **Step 1.6: DO NOT COMMIT.** Move to Task 2.

---

## Task 2: Remove auto-update

**Files:**
- Delete: `crates/auto_update/`
- Delete: `crates/auto_update_ui/`
- Modify: `/Users/memo/Desktop/zed/Cargo.toml`
- Modify: every `Cargo.toml` that depends on `auto_update` or `auto_update_ui`
- Modify: `crates/zed/src/main.rs` (likely initializes auto_update)
- Modify: `crates/zed/src/zed/app_menus.rs` (remove "Check for Updates" menu entry)
- Modify: `assets/keymaps/default-*.json` (remove auto_update action bindings if any)

- [ ] **Step 2.1: Enumerate**

```
Grep pattern: "auto_update(_ui)?\\s*=\\s*\\{\\s*workspace" path: . glob: "**/Cargo.toml"
Grep pattern: "use auto_update(_ui)?(::|;)" path: . glob: "**/*.rs"
Grep pattern: "auto_update::" path: . glob: "**/*.rs"
Grep pattern: "AutoUpdate" path: . glob: "**/*.rs"
Grep pattern: "CheckForUpdates|Check for Updates" path: . glob: "**/*.{rs,json}"
```

- [ ] **Step 2.2: Remove call sites**

For each call site:
- Remove `use auto_update*` imports.
- Remove initialization calls (likely in `crates/zed/src/main.rs` or `crates/zed/src/zed.rs`).
- Remove menu entries referencing the auto-update commands in `crates/zed/src/zed/app_menus.rs`.
- Remove any settings keys named `auto_update` or `auto_update_extensions` from settings schema files (search `crates/settings*` and `crates/settings_content`).

- [ ] **Step 2.3: Remove from keymap files**

For each `assets/keymaps/default-{macos,linux,windows}.json`:
- Read the file.
- Remove any binding whose action is `auto_update::*` or `zed::CheckForUpdates`.

- [ ] **Step 2.4: Remove from `Cargo.toml` files and delete crates**

Same pattern as Task 1: workspace members, workspace deps, every dependent `Cargo.toml`, then:

```
rm -rf /Users/memo/Desktop/zed/crates/auto_update
rm -rf /Users/memo/Desktop/zed/crates/auto_update_ui
```

- [ ] **Step 2.5: Verify**

```
cargo check --workspace
```

- [ ] **Step 2.6: DO NOT COMMIT.** Move to Task 3.

---

## Task 3: Remove feedback

**Files:**
- Delete: `crates/feedback/`
- Modify: `/Users/memo/Desktop/zed/Cargo.toml`
- Modify: every `Cargo.toml` that depends on `feedback`
- Modify: `crates/zed/src/zed/app_menus.rs` (remove "Give Feedback")
- Modify: `assets/keymaps/default-*.json`

- [ ] **Step 3.1: Enumerate**

```
Grep pattern: "feedback\\s*=\\s*\\{\\s*workspace" path: . glob: "**/Cargo.toml"
Grep pattern: "use feedback(::|;)" path: . glob: "**/*.rs"
Grep pattern: "feedback::" path: . glob: "**/*.rs"
Grep pattern: "GiveFeedback|FileBugReport|RequestFeature" path: . glob: "**/*.{rs,json}"
```

- [ ] **Step 3.2: Remove call sites and menu entries**

- Remove `feedback::init(...)` calls (likely in `crates/zed/src/zed.rs`).
- Remove "Give Feedback", "File Bug Report", "Request Feature" menu entries in `crates/zed/src/zed/app_menus.rs`.
- Remove keymap bindings for these actions.

- [ ] **Step 3.3: Remove from `Cargo.toml`s and delete**

```
rm -rf /Users/memo/Desktop/zed/crates/feedback
```

- [ ] **Step 3.4: Verify**

```
cargo check --workspace
```

- [ ] **Step 3.5: DO NOT COMMIT.** Move to Task 4.

---

## Task 4: Remove cloud LLM provider

**Files:**
- Delete: `crates/language_models/src/provider/cloud.rs`
- Modify: `crates/language_models/src/language_models.rs` (lines around 229–236 — registration call)
- Modify: `crates/language_models/src/provider.rs` or `mod.rs` (remove `pub mod cloud;`)
- Modify: `crates/language_models/Cargo.toml` (remove deps only the cloud provider used: likely `client`, `rpc`-related, `proto`, etc., but keep deps used by other providers)
- Audit: `crates/language_model/` (the data-type crate) for any cloud-only types (e.g. `CloudModel`, plan/billing structs) that can be deleted

- [ ] **Step 4.1: Read the cloud provider and its registration**

```
Read crates/language_models/src/language_models.rs (full file)
Read crates/language_models/src/provider/cloud.rs (first 200 lines for header / public surface)
```

Identify: which types from `crates/language_model/` are cloud-only? Which deps in `crates/language_models/Cargo.toml` are cloud-only?

- [ ] **Step 4.2: Remove the registration call**

In `crates/language_models/src/language_models.rs`, remove:
```rust
registry.register_provider(
    Arc::new(CloudLanguageModelProvider::new(
        user_store,
        client.clone(),
        cx,
    )),
    cx,
);
```
…and the `use crate::provider::cloud::CloudLanguageModelProvider;` import. Remove the `user_store` and `client` parameters from the function signature if cloud was their only consumer; otherwise leave them and the compiler will flag dead params.

- [ ] **Step 4.3: Delete the provider module**

```
rm /Users/memo/Desktop/zed/crates/language_models/src/provider/cloud.rs
```

Remove `pub mod cloud;` from `crates/language_models/src/provider.rs` (or wherever it's declared).

- [ ] **Step 4.4: Remove cloud-only types from `language_model` crate**

For each type identified in 4.1 that was used only by `cloud.rs`:
- Grep for usages across the workspace to confirm no other consumers.
- Delete the type and any associated `impl` blocks.

- [ ] **Step 4.5: Prune `language_models/Cargo.toml`**

Remove deps that are now unused by the remaining providers. Re-run `cargo check -p language_models` to confirm.

- [ ] **Step 4.6: Verify**

```
cargo check --workspace
```

Manually verify (read the registration site) that Anthropic, OpenAI, Ollama, Copilot Chat, Bedrock, etc. providers are still registered.

- [ ] **Step 4.7: DO NOT COMMIT.** Move to Task 5.

---

## Task 5: Remove Zed-hosted edit predictions (Zeta)

**Files:**
- Modify: `crates/edit_prediction/` (remove Zeta provider; keep Copilot/Supermaven providers)
- Audit and possibly delete: any `zeta*` files inside `crates/edit_prediction/src/`
- Modify: `crates/edit_prediction/Cargo.toml` (drop `client`, `rpc`-related, `credentials_provider` deps if only Zeta used them)
- Modify: settings schema files that name `zed` as an edit-prediction provider option

- [ ] **Step 5.1: Map the providers**

```
Read crates/edit_prediction/src/edit_prediction.rs (or lib.rs root)
Grep pattern: "provider\\s*:" path: crates/edit_prediction/src
Grep pattern: "Zeta|zeta" path: crates/edit_prediction
```

Identify each provider variant and which file implements it. Goal: keep Copilot and Supermaven, remove Zeta (and "none"/"off" if it exists, keep that).

- [ ] **Step 5.2: Remove the Zeta provider variant**

In the provider enum / settings, remove the Zeta arm. In the `match settings.provider` site (around line 968 per the spec verification), remove the Zeta branch. Delete the Zeta-specific module files.

- [ ] **Step 5.3: Remove settings options for the Zed provider**

Search settings schema for the edit-prediction provider field's enum values. Remove `"zed"` from the allowed values and from any docs / examples.

- [ ] **Step 5.4: Prune `Cargo.toml`**

Drop `client` and other Zeta-only deps from `crates/edit_prediction/Cargo.toml`. Run `cargo check -p edit_prediction`.

- [ ] **Step 5.5: Verify**

```
cargo check --workspace
```

- [ ] **Step 5.6: DO NOT COMMIT.** Move to Task 6.

---

## Task 6: Remove collaboration

This is the largest task. It removes channels, calls, shared projects, screen sharing, the collab panel, and the notifications panel.

**Files:**
- Delete: `crates/collab/`
- Delete: `crates/collab_ui/`
- Delete: `crates/call/`
- Delete: `crates/channel/`
- Delete: `crates/livekit_client/`
- Delete: `crates/livekit_api/`
- Delete: `crates/notifications/` (depends on `channel`, `client`, `rpc` — collab-driven per spec verification)
- Modify: `/Users/memo/Desktop/zed/Cargo.toml`
- Modify: every `Cargo.toml` depending on the above
- Modify: `crates/workspace/` — remove shared-project / follower / participant code paths
- Modify: `crates/title_bar/` — remove collaborator avatars, "share project" buttons, calls UI
- Modify: `crates/project/` — remove `RemoteProject` / shared-project hosting code (if any survives without `collab`)
- Modify: `crates/zed/src/zed.rs` and `app_menus.rs` — remove collab-related init calls and menu items
- Modify: `assets/keymaps/default-*.json` — remove channel/call/shared-project bindings

- [ ] **Step 6.1: Enumerate dependents**

```
Grep pattern: "(collab|collab_ui|call|channel|livekit_client|livekit_api|notifications)\\s*=\\s*\\{\\s*workspace" path: . glob: "**/Cargo.toml"
Grep pattern: "use (collab|collab_ui|call|channel|livekit_client|livekit_api|notifications)(::|;)" path: . glob: "**/*.rs"
```

Record all dependent crates. Expect: `workspace`, `title_bar`, `project`, `zed`, `feedback` (already deleted), `editor`, and others.

- [ ] **Step 6.2: Remove `notifications` consumers**

The notifications panel is collab-driven. For each `use notifications::*` site:
- Remove the import.
- Remove `notifications::init(...)` calls.
- Remove the panel registration in `crates/zed/src/zed.rs` (likely `workspace.add_panel::<NotificationPanel>(...)`).
- Remove notification dock activation actions from keymaps/menus.

- [ ] **Step 6.3: Remove `collab_ui` consumers**

In `crates/zed/src/zed.rs` (or wherever `collab_ui::init` is called):
- Remove the init call.
- Remove panel registrations for `CollabPanel`, `ChatPanel`.

In `crates/title_bar/src/title_bar.rs`:
- Remove the collaborator faces / participants section.
- Remove "Share Project" / "Mute Mic" / "Share Screen" buttons.
- Remove `call::*` and `collab_ui::*` imports.

In `crates/workspace/`:
- Remove `Follow`, `FollowNextCollaborator`, `Unfollow`, `ToggleFollow` actions and their handlers.
- Remove "follower" rendering paths in pane / item code (search for `leader_id`, `Follower`, `Following`).
- Remove `RemoteProject` / shared-project lifecycle code.

In `crates/project/`:
- Search for `client.rs`/`call.rs`/`collab_*` usages. Remove project sharing entry points (`share`, `unshare`, `Project::remote`).

- [ ] **Step 6.4: Remove keymap and menu entries**

For each `assets/keymaps/default-{macos,linux,windows}.json`:
- Remove bindings for: `collab_panel::*`, `chat_panel::*`, `channel::*`, `call::*`, `notification_panel::*`, `workspace::Follow*`, `workspace::Unfollow`, `workspace::ToggleFollow`, `workspace::ShareProject`, `workspace::UnshareProject`.

In `crates/zed/src/zed/app_menus.rs`:
- Remove menu entries that reference any of the above actions.

- [ ] **Step 6.5: Remove from `Cargo.toml`s**

For every `Cargo.toml` from step 6.1: remove the matching `workspace = true` dep lines. Then in root `Cargo.toml`:
- Remove from `members`: `crates/collab`, `crates/collab_ui`, `crates/call`, `crates/channel`, `crates/livekit_client`, `crates/livekit_api`, `crates/notifications`.
- Remove from `[workspace.dependencies]`: the matching entries.

- [ ] **Step 6.6: Delete the crates**

```
rm -rf /Users/memo/Desktop/zed/crates/collab
rm -rf /Users/memo/Desktop/zed/crates/collab_ui
rm -rf /Users/memo/Desktop/zed/crates/call
rm -rf /Users/memo/Desktop/zed/crates/channel
rm -rf /Users/memo/Desktop/zed/crates/livekit_client
rm -rf /Users/memo/Desktop/zed/crates/livekit_api
rm -rf /Users/memo/Desktop/zed/crates/notifications
```

- [ ] **Step 6.7: Verify**

```
cargo check --workspace
```

Expect many errors on the first run — fix in waves: remove dead imports, then dead struct fields, then dead handlers, until clean.

- [ ] **Step 6.8: DO NOT COMMIT.** Move to Task 7.

---

## Task 7: Remove sign-in UI

**Files:**
- Modify: `crates/client/src/client.rs` lines 91–101 (remove `SignIn`, `SignOut`, `Reconnect` action defs; keep the `actions!` macro if other actions remain, otherwise delete the block)
- Modify: `crates/title_bar/src/title_bar.rs` (remove `render_sign_in_button`, line 1140; remove the call site at line 325; remove "account" / user-avatar code)
- Modify: any welcome screen / onboarding pages that prompt sign-in (search `crates/welcome/`, `crates/onboarding/` if they exist)
- Modify: `crates/edit_prediction_ui/src/edit_prediction_button.rs` (remove sign-in branch — should be dead since cloud LLM and Zeta are already removed, but verify and delete)
- Modify: settings schema — remove `title_bar.show_sign_in` from settings
- Modify: `assets/keymaps/default-*.json` (remove `client::SignIn`, `client::SignOut`, `client::Reconnect` bindings)
- Modify: `crates/zed/src/zed/app_menus.rs`

- [ ] **Step 7.1: Enumerate**

```
Grep pattern: "SignIn|SignOut|sign_in|signed_in|sign in" path: . glob: "**/*.rs"
Grep pattern: "show_sign_in" path: .
Grep pattern: "client::(SignIn|SignOut|Reconnect)" path: . glob: "**/*.json"
```

- [ ] **Step 7.2: Remove the `SignIn` / `SignOut` / `Reconnect` actions**

In `crates/client/src/client.rs` lines 91–101: remove the three action definitions (or the whole `actions!` block if those were the only three).

- [ ] **Step 7.3: Remove handlers**

For each handler registered for these actions (`on_action(|_: &SignIn, ...|`), remove the handler. Likely sites:
- `crates/title_bar/src/title_bar.rs`
- `crates/workspace/`
- `crates/zed/src/zed.rs`

- [ ] **Step 7.4: Remove sign-in UI rendering**

In `crates/title_bar/src/title_bar.rs`:
- Delete `render_sign_in_button` function (around line 1140).
- Delete its call site (around line 325).
- Delete user-avatar / account-menu code (search `current_user`, `user_avatar`).

In any welcome / onboarding crates: remove sign-in prompts.

In `crates/edit_prediction_ui/src/edit_prediction_button.rs`: remove the sign-in CTA branch.

- [ ] **Step 7.5: Remove `show_sign_in` setting**

Search `crates/settings_content/`, `crates/settings_ui/`, and any other settings schema files for `show_sign_in`. Delete the field, its default, and its UI row.

- [ ] **Step 7.6: Remove keymap bindings**

For each `assets/keymaps/default-{macos,linux,windows}.json`: remove bindings for `client::SignIn`, `client::SignOut`, `client::Reconnect`.

- [ ] **Step 7.7: Verify**

```
cargo check --workspace
```

- [ ] **Step 7.8: DO NOT COMMIT.** Move to Task 8.

---

## Task 8: Gut the `client` crate auth/RPC path

By this point, every consumer of `UserStore`, `current_user`, `Status`, and the RPC session should be gone. This task confirms that and deletes the dead code.

**Files:**
- Modify (heavily) or delete in part: `crates/client/src/`
  - `user.rs` (UserStore)
  - `client.rs` (RPC session lifecycle, OAuth dispatch)
  - Any `telemetry.rs` (already deleted in Task 1, confirm no stragglers)
  - Keychain credential storage
- Delete: `crates/oauth_callback_server/`
- Delete or gut: `crates/rpc/src/auth.rs` (RSA token handshake)
- Audit: `crates/rpc/` — if nothing remains after auth/session removal, delete the whole crate
- Modify: `crates/client/Cargo.toml` (drop deps no longer needed)
- Audit: env vars — remove `ZED_IMPERSONATE`, `ZED_WEB_LOGIN` handling (in `crates/client/src/client.rs`)
- Keep: HTTP client surface used by extensions

- [ ] **Step 8.1: Confirm consumers are gone**

```
Grep pattern: "current_user\\(" path: . glob: "**/*.rs"
Grep pattern: "UserStore" path: . glob: "**/*.rs"
Grep pattern: "client\\.status\\(\\)" path: . glob: "**/*.rs"
Grep pattern: "ZED_IMPERSONATE|ZED_WEB_LOGIN" path: .
```

If any usages remain in non-`client` crates, they are leftovers from earlier tasks — remove them now (likely small).

- [ ] **Step 8.2: Read the `client` crate surface**

```
Read crates/client/src/client.rs
Read crates/client/src/user.rs
```

Decide for each `pub` item: kept (HTTP-related, http_client wrapping, settings) or removed (UserStore, sign-in, RPC connection, OAuth, telemetry hooks already gone, keychain).

- [ ] **Step 8.3: Delete `UserStore` and friends**

In `crates/client/src/user.rs`:
- Delete `UserStore`, `User`, `Collaborator`, `Contact`, `ParticipantIndex`, anything else collab- or auth-only.
- Keep nothing if the entire module is auth/collab — delete the file and the `mod user;` declaration in `client.rs`.

- [ ] **Step 8.4: Delete RPC session and OAuth from `client.rs`**

In `crates/client/src/client.rs`:
- Delete `SignIn`/`SignOut`/`Reconnect` action handlers (already gone if Task 7 done; verify).
- Delete `establish_connection`, `authenticate_and_connect`, `set_status`, the `Status` enum.
- Delete the OAuth dispatcher and the keychain credential code.
- Delete the `ZED_IMPERSONATE` and `ZED_WEB_LOGIN` env var handling.
- Delete the `credentials_url`, `server_url` settings entries — search `crates/settings_content` and remove these too.

Keep: the HTTP client (or whatever shim the rest of Zed uses to reach `zed.dev` for extensions). If extensions use `http_client::HttpClient` directly, `client::Client` may be deletable entirely — confirm with:

```
Grep pattern: "client::Client" path: . glob: "**/*.rs"
```

If `client::Client` has no remaining users, delete it. If some users remain (likely the extension marketplace), keep a slim version that just exposes `http_client`.

- [ ] **Step 8.5: Delete `oauth_callback_server`**

```
rm -rf /Users/memo/Desktop/zed/crates/oauth_callback_server
```

Remove from workspace members + dependencies. Remove from `client/Cargo.toml`.

- [ ] **Step 8.6: Gut or delete `rpc`**

Read `crates/rpc/src/` and identify what's used outside of the deleted features.

```
Grep pattern: "rpc::" path: . glob: "**/*.rs"
Grep pattern: "use rpc(::|;)" path: . glob: "**/*.rs"
```

If nothing outside of (already-deleted) collab/client uses `rpc`, delete the whole crate. Otherwise, delete `crates/rpc/src/auth.rs` and any modules used only by collab.

```
# If rpc has no remaining consumers:
rm -rf /Users/memo/Desktop/zed/crates/rpc
```

Remove from workspace members + dependencies if deleted.

- [ ] **Step 8.7: Prune `client/Cargo.toml`**

Remove `rpc`, `livekit_*`, `credentials_provider`, anything else now unused.

- [ ] **Step 8.8: Verify**

```
cargo check --workspace
```

- [ ] **Step 8.9: DO NOT COMMIT.** Move to Task 9.

---

## Task 9: Settings sync cleanup

Per spec verification, no separate sync subsystem exists — sync was implicit in the cloud account path, which Task 8 removed. This task is a verification pass.

- [ ] **Step 9.1: Search for any remaining sync references**

```
Grep pattern: "(settings_sync|sync_settings|SyncSettings|settings_store_sync)" path: . glob: "**/*.rs"
Grep pattern: "(settings_sync|sync_settings)" path: . glob: "**/*.{json,toml}"
```

If anything is found, remove it. If nothing is found, the phase is complete.

- [ ] **Step 9.2: Verify**

```
cargo check --workspace
```

- [ ] **Step 9.3: DO NOT COMMIT.** Move to Task 10.

---

## Task 10: Final cleanup pass

**Files:**
- `/Users/memo/Desktop/zed/Cargo.toml` (workspace members + deps)
- Every keymap and menu file
- Settings schema files
- Doc/example files

- [ ] **Step 10.1: Confirm workspace is clean**

```
cargo tree -e=workspace 2>&1 | grep -E "(telemetry|telemetry_events|auto_update|auto_update_ui|feedback|collab|collab_ui|call|channel|livekit_client|livekit_api|notifications|oauth_callback_server)"
```

Expected: empty output. If any of these appear, a `Cargo.toml` still references them — fix it.

- [ ] **Step 10.2: Confirm removed actions are gone from keymaps**

```
Grep pattern: "client::(SignIn|SignOut|Reconnect)" path: assets
Grep pattern: "auto_update::" path: assets
Grep pattern: "(collab_panel|chat_panel|notification_panel|channel)::" path: assets
Grep pattern: "feedback::" path: assets
Grep pattern: "workspace::(Follow|Unfollow|ToggleFollow|ShareProject|UnshareProject)" path: assets
```

Expected: all empty. Remove any stragglers.

- [ ] **Step 10.3: Confirm removed actions are gone from menus**

```
Grep pattern: "(SignIn|SignOut|CheckForUpdates|GiveFeedback|FileBugReport|Follow|ShareProject)" path: crates/zed/src/zed/app_menus.rs
```

Expected: empty. Remove any stragglers.

- [ ] **Step 10.4: Confirm removed settings keys are gone**

```
Grep pattern: "(show_sign_in|server_url|credentials_url|auto_update|telemetry|diagnostics)" path: crates/settings_content
```

Each match — decide: kept (e.g., a non-Zed telemetry like LSP diagnostics) or removed. Remove the dead ones.

- [ ] **Step 10.5: Confirm removed env vars are gone**

```
Grep pattern: "ZED_IMPERSONATE|ZED_WEB_LOGIN" path: .
```

Expected: empty.

- [ ] **Step 10.6: Run clippy**

```
./script/clippy
```

Expected: clean. Fix anything new.

- [ ] **Step 10.7: Run tests**

```
cargo nextest run --workspace
```

Expected: passes. Failures are acceptable only if they were tests for deleted features that didn't get deleted with their crate — in which case delete those tests now.

- [ ] **Step 10.8: Final manual checklist (cannot be scripted)**

Confirm with the user before claiming done. The user (not the agent) must launch Zed and verify:
- No "Sign in" UI anywhere (title bar, command palette, welcome page, agent panel).
- No collab / channels / calls / shared projects / notifications panel.
- Cloud LLM provider missing from agent settings; Anthropic/OpenAI/Ollama/Copilot/Supermaven still listed.
- No "Check for Updates" / "Give Feedback" in command palette.
- Extensions panel lists and installs extensions.
- SSH remote dev still connects.

- [ ] **Step 10.9: DO NOT COMMIT.** Report to user: "All phases complete. Working tree has uncommitted changes ready for your review. I did not commit anything per your instruction."

---

## Self-Review (filled in by plan author)

**Spec coverage:**
- Sign-in UI removal — Task 7 ✓
- Auth plumbing removal — Task 8 ✓
- `client` gut — Task 8 ✓
- Collaboration — Task 6 ✓
- AI tied to Zed account: cloud LLM — Task 4; Zeta — Task 5 ✓
- Telemetry — Task 1 ✓
- Auto-update — Task 2 ✓
- Feedback — Task 3 ✓
- Settings sync — Task 9 (verification-only, since no separate subsystem) ✓
- Preserved features (extensions, remote dev, Copilot, BYO-key AI) — preserved by being absent from any deletion task ✓
- Definition of done conditions — Task 10 ✓
- "No commits" — repeated as a ground rule and at the end of every task ✓

**Placeholder scan:** No TBD/TODO. The plan uses Grep enumeration as the "discovery" step within each task, which is the appropriate pattern for removal work (you cannot pre-list every line to delete without re-doing the implementation in the plan).

**Type consistency:** Crate names verified by the Explore agent. Line numbers cited where they were verified (cloud provider registration at 229–236, SignIn actions at 91–101, render_sign_in_button at 1140, title bar caller at 325). All `rm -rf` paths are absolute and correspond to verified-existing crates.
