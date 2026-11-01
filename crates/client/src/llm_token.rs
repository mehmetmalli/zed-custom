//! Stub of the cloud LLM token refresh subsystem. After the Zed cloud LLM
//! provider was removed, no tokens are issued. The `RefreshLlmTokenListener`
//! type is retained because many call sites still register it during init;
//! all operations here are no-ops.

use super::{Client, UserStore};
use gpui::{App, AppContext as _, Entity, EventEmitter, Global, ReadGlobal as _};
use std::sync::Arc;

pub trait NeedsLlmTokenRefresh {
    fn needs_llm_token_refresh(&self) -> bool;
}

impl NeedsLlmTokenRefresh for http_client::Response<http_client::AsyncBody> {
    fn needs_llm_token_refresh(&self) -> bool {
        false
    }
}

pub struct LlmTokenRefreshedEvent;

struct GlobalRefreshLlmTokenListener(Entity<RefreshLlmTokenListener>);

impl Global for GlobalRefreshLlmTokenListener {}

pub struct RefreshLlmTokenListener;

impl EventEmitter<LlmTokenRefreshedEvent> for RefreshLlmTokenListener {}

impl RefreshLlmTokenListener {
    pub fn register(_client: Arc<Client>, _user_store: Entity<UserStore>, cx: &mut App) {
        let listener = cx.new(|_| RefreshLlmTokenListener);
        cx.set_global(GlobalRefreshLlmTokenListener(listener));
    }

    pub fn global(cx: &App) -> Entity<Self> {
        GlobalRefreshLlmTokenListener::global(cx).0.clone()
    }
}

