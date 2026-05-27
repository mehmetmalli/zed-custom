use client::{Client, UserStore};
use gpui::{App, Entity};
use std::sync::Arc;
use web_search::WebSearchRegistry;

pub fn init(_client: Arc<Client>, _user_store: Entity<UserStore>, _cx: &mut App) {
    let _ = WebSearchRegistry::global; // keep import live
}
