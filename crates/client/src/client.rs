mod llm_token;
pub mod telemetry;
pub mod user;
pub mod zed_urls;

use anyhow::Result;
use futures::{
    Future, FutureExt,
    future::BoxFuture,
    stream::BoxStream,
};
use gpui::{App, AsyncApp, Entity, Global, WeakEntity};
use http_client::{HttpClientWithUrl, read_proxy_from_env};
use parking_lot::Mutex;
use rpc::proto::{AnyTypedEnvelope, EnvelopedMessage, PeerId, RequestMessage};
use serde::Deserialize;
use settings::{RegisterSetting, Settings};
use std::{
    any::TypeId,
    marker::PhantomData,
    sync::{
        Arc, LazyLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
use url::Url;
use util::ResultExt;

pub use llm_token::*;
pub use rpc::*;
pub use telemetry::Telemetry;
pub use user::*;

/// Connection status of the (removed) zed.dev RPC session. The slim client
/// is permanently `SignedOut`; the enum is kept so consumers that exhaustively
/// match on connection state still compile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    SignedOut,
    Authenticating,
    Authenticated,
    Connecting,
    Connected { peer_id: PeerId, connection_id: ConnectionId },
    ConnectionError,
    ConnectionLost,
    Reauthenticating,
    Reauthenticated,
    Reconnecting,
    ReconnectionError { next_reconnection: std::time::Instant },
    UpgradeRequired,
    AuthenticationError,
}

impl Status {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }

    pub fn is_signed_out(&self) -> bool {
        matches!(self, Self::SignedOut | Self::UpgradeRequired)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct ChannelId(pub u64);

impl std::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

static ZED_SERVER_URL: LazyLock<Option<String>> =
    LazyLock::new(|| std::env::var("ZED_SERVER_URL").ok());

pub static ZED_APP_PATH: LazyLock<Option<std::path::PathBuf>> =
    LazyLock::new(|| std::env::var("ZED_APP_PATH").ok().map(std::path::PathBuf::from));

pub static ZED_ALWAYS_ACTIVE: LazyLock<bool> =
    LazyLock::new(|| std::env::var("ZED_ALWAYS_ACTIVE").is_ok_and(|e| !e.is_empty()));

const DEFAULT_ZED_SERVER_URL: &str = "https://zed.dev";

#[derive(Deserialize, RegisterSetting)]
pub struct ClientSettings {
    pub server_url: String,
}

impl Settings for ClientSettings {
    fn from_settings(_content: &settings::SettingsContent) -> Self {
        let server_url = ZED_SERVER_URL
            .clone()
            .unwrap_or_else(|| DEFAULT_ZED_SERVER_URL.to_string());
        Self { server_url }
    }
}

#[derive(Deserialize, Default, RegisterSetting)]
pub struct ProxySettings {
    pub proxy: Option<String>,
}

impl ProxySettings {
    pub fn proxy_url(&self) -> Option<Url> {
        self.proxy
            .as_deref()
            .map(str::trim)
            .filter(|input| !input.is_empty())
            .and_then(|input| {
                input
                    .parse::<Url>()
                    .inspect_err(|e| log::error!("Error parsing proxy settings: {}", e))
                    .ok()
            })
            .or_else(read_proxy_from_env)
    }
}

impl Settings for ProxySettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        Self {
            proxy: content
                .proxy
                .as_deref()
                .map(str::trim)
                .filter(|proxy| !proxy.is_empty())
                .map(ToOwned::to_owned),
        }
    }
}

pub fn init(_client: &Arc<Client>, _cx: &mut App) {}

pub type MessageToClientHandler = Box<dyn Fn() + Send + Sync + 'static>;

struct GlobalClient(Arc<Client>);

impl Global for GlobalClient {}

/// Slim HTTP-only client. After the Zed sign-in / collab removal, this no
/// longer maintains an RPC session, OAuth credentials, telemetry, or cloud
/// websocket. It exists primarily so existing call sites (extension host,
/// project handler registration, etc.) keep working as no-ops, and so the
/// HTTP client used for non-authenticated zed.dev endpoints (extension
/// marketplace) can be shared.
pub struct Client {
    id: AtomicU64,
    peer: Arc<Peer>,
    http: Arc<HttpClientWithUrl>,
    handler_set: Mutex<ProtoMessageHandlerSet>,
    _status: postage::watch::Sender<Status>,
    status_rx: postage::watch::Receiver<Status>,
    telemetry: Arc<Telemetry>,
}

pub enum Subscription {
    Entity {
        client: Weak<Client>,
        id: (TypeId, u64),
    },
    Message {
        client: Weak<Client>,
        id: TypeId,
    },
}

impl Drop for Subscription {
    fn drop(&mut self) {
        match self {
            Subscription::Entity { client, id } => {
                if let Some(client) = client.upgrade() {
                    let mut state = client.handler_set.lock();
                    let _ = state.entities_by_type_and_remote_id.remove(id);
                }
            }
            Subscription::Message { client, id } => {
                if let Some(client) = client.upgrade() {
                    let mut state = client.handler_set.lock();
                    let _ = state.entity_types_by_message_type.remove(id);
                    let _ = state.message_handlers.remove(id);
                }
            }
        }
    }
}

pub struct PendingEntitySubscription<T: 'static> {
    client: Arc<Client>,
    remote_id: u64,
    _entity_type: PhantomData<T>,
    consumed: bool,
}

impl<T: 'static> PendingEntitySubscription<T> {
    pub fn set_entity(mut self, entity: &Entity<T>, cx: &AsyncApp) -> Subscription {
        self.consumed = true;
        let mut handlers = self.client.handler_set.lock();
        let id = (TypeId::of::<T>(), self.remote_id);
        let Some(EntityMessageSubscriber::Pending(messages)) =
            handlers.entities_by_type_and_remote_id.remove(&id)
        else {
            unreachable!()
        };

        handlers.entities_by_type_and_remote_id.insert(
            id,
            EntityMessageSubscriber::Entity {
                handle: entity.downgrade().into(),
            },
        );
        drop(handlers);
        for message in messages {
            let client_id = self.client.id();
            let type_name = message.payload_type_name();
            let sender_id = message.original_sender_id();
            log::debug!(
                "handling queued rpc message. client_id:{}, sender_id:{:?}, type:{}",
                client_id,
                sender_id,
                type_name
            );
            self.client.handle_message(message, cx);
        }
        Subscription::Entity {
            client: Arc::downgrade(&self.client),
            id,
        }
    }
}

impl<T: 'static> Drop for PendingEntitySubscription<T> {
    fn drop(&mut self) {
        if !self.consumed {
            let mut state = self.client.handler_set.lock();
            if let Some(EntityMessageSubscriber::Pending(messages)) = state
                .entities_by_type_and_remote_id
                .remove(&(TypeId::of::<T>(), self.remote_id))
            {
                for message in messages {
                    log::info!("unhandled message {}", message.payload_type_name());
                }
            }
        }
    }
}

impl Client {
    pub fn new(http: Arc<HttpClientWithUrl>, cx: &mut App) -> Arc<Self> {
        let (status, status_rx) = postage::watch::channel_with(Status::SignedOut);
        Arc::new(Self {
            id: AtomicU64::new(0),
            peer: Peer::new(0),
            http: http.clone(),
            handler_set: Default::default(),
            _status: status,
            status_rx,
            telemetry: Telemetry::new(http, cx),
        })
    }

    pub fn production(cx: &mut App) -> Arc<Self> {
        let http = Arc::new(HttpClientWithUrl::new_url(
            cx.http_client(),
            &ClientSettings::get_global(cx).server_url,
            cx.http_client().proxy().cloned(),
        ));
        Self::new(http, cx)
    }

    pub fn id(&self) -> u64 {
        self.id.load(Ordering::SeqCst)
    }

    pub fn http_client(&self) -> Arc<HttpClientWithUrl> {
        self.http.clone()
    }

    pub fn telemetry(&self) -> &Arc<Telemetry> {
        &self.telemetry
    }

    /// Returns a receiver that always reports `SignedOut`. The Zed sign-in
    /// flow has been removed, so the connection state never changes.
    pub fn status(&self) -> postage::watch::Receiver<Status> {
        self.status_rx.clone()
    }

    pub fn set_id(&self, id: u64) -> &Self {
        self.id.store(id, Ordering::SeqCst);
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn teardown(&self) {
        self.handler_set.lock().clear();
        self.peer.teardown();
    }

    pub fn global(cx: &App) -> Arc<Self> {
        cx.global::<GlobalClient>().0.clone()
    }

    pub fn set_global(client: Arc<Client>, cx: &mut App) {
        cx.set_global(GlobalClient(client))
    }

    /// Always returns `None`. The Zed sign-in flow has been removed.
    pub fn user_id(&self) -> Option<u64> {
        None
    }

    /// Always returns `None`. RPC sessions to zed.dev have been removed.
    pub fn peer_id(&self) -> Option<PeerId> {
        None
    }

    pub fn subscribe_to_entity<T>(
        self: &Arc<Self>,
        remote_id: u64,
    ) -> Result<PendingEntitySubscription<T>>
    where
        T: 'static,
    {
        let id = (TypeId::of::<T>(), remote_id);

        let mut state = self.handler_set.lock();
        anyhow::ensure!(
            !state.entities_by_type_and_remote_id.contains_key(&id),
            "already subscribed to entity"
        );

        state
            .entities_by_type_and_remote_id
            .insert(id, EntityMessageSubscriber::Pending(Default::default()));

        Ok(PendingEntitySubscription {
            client: self.clone(),
            remote_id,
            consumed: false,
            _entity_type: PhantomData,
        })
    }

    #[track_caller]
    pub fn add_message_handler<M, E, H, F>(
        self: &Arc<Self>,
        entity: WeakEntity<E>,
        handler: H,
    ) -> Subscription
    where
        M: EnvelopedMessage,
        E: 'static,
        H: 'static + Sync + Fn(Entity<E>, TypedEnvelope<M>, AsyncApp) -> F + Send + Sync,
        F: 'static + Future<Output = Result<()>>,
    {
        self.add_message_handler_impl(entity, move |entity, message, _, cx| {
            handler(entity, message, cx)
        })
    }

    fn add_message_handler_impl<M, E, H, F>(
        self: &Arc<Self>,
        entity: WeakEntity<E>,
        handler: H,
    ) -> Subscription
    where
        M: EnvelopedMessage,
        E: 'static,
        H: 'static
            + Sync
            + Fn(Entity<E>, TypedEnvelope<M>, AnyProtoClient, AsyncApp) -> F
            + Send
            + Sync,
        F: 'static + Future<Output = Result<()>>,
    {
        let message_type_id = TypeId::of::<M>();
        let mut state = self.handler_set.lock();
        state
            .entities_by_message_type
            .insert(message_type_id, entity.into());

        let prev_handler = state.message_handlers.insert(
            message_type_id,
            Arc::new(move |subscriber, envelope, client, cx| {
                let subscriber = subscriber.downcast::<E>().unwrap();
                let envelope = envelope.into_any().downcast::<TypedEnvelope<M>>().unwrap();
                handler(subscriber, *envelope, client, cx).boxed_local()
            }),
        );
        if prev_handler.is_some() {
            let location = std::panic::Location::caller();
            panic!(
                "{}:{} registered handler for the same message {} twice",
                location.file(),
                location.line(),
                std::any::type_name::<M>()
            );
        }

        Subscription::Message {
            client: Arc::downgrade(self),
            id: message_type_id,
        }
    }

    pub fn add_request_handler<M, E, H, F>(
        self: &Arc<Self>,
        entity: WeakEntity<E>,
        handler: H,
    ) -> Subscription
    where
        M: RequestMessage,
        E: 'static,
        H: 'static + Sync + Fn(Entity<E>, TypedEnvelope<M>, AsyncApp) -> F + Send + Sync,
        F: 'static + Future<Output = Result<M::Response>>,
    {
        self.add_message_handler_impl(entity, move |handle, envelope, this, cx| {
            Self::respond_to_request(envelope.receipt(), handler(handle, envelope, cx), this)
        })
    }

    async fn respond_to_request<T: RequestMessage, F: Future<Output = Result<T::Response>>>(
        receipt: Receipt<T>,
        response: F,
        client: AnyProtoClient,
    ) -> Result<()> {
        match response.await {
            Ok(response) => {
                client.send_response(receipt.message_id, response)?;
                Ok(())
            }
            Err(error) => {
                client.send_response(receipt.message_id, error.to_proto())?;
                Err(error)
            }
        }
    }

    /// Sign-in has been removed from Zed. This is a no-op kept so that
    /// callers waiting on the request queue do not break.
    pub fn request_sign_out(&self) {}

    pub fn send<T: EnvelopedMessage>(&self, _message: T) -> Result<()> {
        anyhow::bail!("not connected: zed.dev sign-in has been removed");
    }

    pub fn request<T: RequestMessage>(
        &self,
        _request: T,
    ) -> impl Future<Output = Result<T::Response>> + use<T> {
        async {
            anyhow::bail!("not connected: zed.dev sign-in has been removed");
        }
    }

    pub fn request_stream<T: RequestMessage>(
        &self,
        _request: T,
    ) -> impl Future<Output = Result<BoxStream<'static, Result<T::Response>>>> + use<T> {
        async {
            anyhow::bail!("not connected: zed.dev sign-in has been removed");
        }
    }

    pub fn request_envelope<T: RequestMessage>(
        &self,
        _request: T,
    ) -> impl Future<Output = Result<TypedEnvelope<T::Response>>> + use<T> {
        async {
            anyhow::bail!("not connected: zed.dev sign-in has been removed");
        }
    }

    pub fn request_dynamic(
        &self,
        _envelope: proto::Envelope,
        _request_type: &'static str,
    ) -> impl Future<Output = Result<proto::Envelope>> + use<> {
        async {
            anyhow::bail!("not connected: zed.dev sign-in has been removed");
        }
    }

    fn handle_message(self: &Arc<Client>, message: Box<dyn AnyTypedEnvelope>, cx: &AsyncApp) {
        let sender_id = message.sender_id();
        let request_id = message.message_id();
        let type_name = message.payload_type_name();
        let original_sender_id = message.original_sender_id();

        if let Some(future) = ProtoMessageHandlerSet::handle_message(
            &self.handler_set,
            message,
            self.clone().into(),
            cx.clone(),
        ) {
            let client_id = self.id();
            log::debug!(
                "rpc message received. client_id:{}, sender_id:{:?}, type:{}",
                client_id,
                original_sender_id,
                type_name
            );
            cx.spawn(async move |_| match future.await {
                Ok(()) => {
                    log::debug!("rpc message handled. client_id:{client_id}, sender_id:{original_sender_id:?}, type:{type_name}");
                }
                Err(error) => {
                    log::error!("error handling message. client_id:{client_id}, sender_id:{original_sender_id:?}, type:{type_name}, error:{error:#}");
                }
            })
            .detach();
        } else {
            log::info!("unhandled message {}", type_name);
            self.peer
                .respond_with_unhandled_message(sender_id.into(), request_id, type_name)
                .log_err();
        }
    }

    pub fn add_message_to_client_handler(
        self: &Arc<Client>,
        _handler: impl Fn() + Send + Sync + 'static,
    ) {
    }
}

impl ProtoClient for Client {
    fn request(
        &self,
        _envelope: proto::Envelope,
        _request_type: &'static str,
    ) -> BoxFuture<'static, Result<proto::Envelope>> {
        async { anyhow::bail!("not connected: zed.dev sign-in has been removed") }.boxed()
    }

    fn request_stream(
        &self,
        _envelope: proto::Envelope,
        _request_type: &'static str,
    ) -> BoxFuture<'static, Result<BoxStream<'static, Result<proto::Envelope>>>> {
        async { anyhow::bail!("not connected: zed.dev sign-in has been removed") }.boxed()
    }

    fn send(&self, _envelope: proto::Envelope, _message_type: &'static str) -> Result<()> {
        anyhow::bail!("not connected: zed.dev sign-in has been removed");
    }

    fn send_response(&self, _envelope: proto::Envelope, _message_type: &'static str) -> Result<()> {
        anyhow::bail!("not connected: zed.dev sign-in has been removed");
    }

    fn message_handler_set(&self) -> &Mutex<ProtoMessageHandlerSet> {
        &self.handler_set
    }

    fn is_via_collab(&self) -> bool {
        true
    }

    fn has_wsl_interop(&self) -> bool {
        false
    }
}

/// prefix for the zed:// url scheme
pub const ZED_URL_SCHEME: &str = "zed";

/// A parsed Zed link that can be handled internally by the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZedLink {
    /// Join a channel: `zed.dev/channel/channel-name-123` or `zed://channel/channel-name-123`
    Channel { channel_id: u64 },
    /// Open channel notes: `zed.dev/channel/channel-name-123/notes` or with heading `notes#heading`
    ChannelNotes {
        channel_id: u64,
        heading: Option<String>,
    },
}

/// Parses the given link into a Zed link.
///
/// Returns a [`Some`] containing the parsed link if the link is a recognized Zed link
/// that should be handled internally by the application.
/// Returns [`None`] for links that should be opened in the browser.
pub fn parse_zed_link(link: &str, cx: &App) -> Option<ZedLink> {
    let server_url = &ClientSettings::get_global(cx).server_url;
    let path = link
        .strip_prefix(server_url)
        .and_then(|result| result.strip_prefix('/'))
        .or_else(|| {
            link.strip_prefix(ZED_URL_SCHEME)
                .and_then(|result| result.strip_prefix("://"))
        })?;

    let mut parts = path.split('/');

    if parts.next() != Some("channel") {
        return None;
    }

    let slug = parts.next()?;
    let id_str = slug.split('-').next_back()?;
    let channel_id = id_str.parse::<u64>().ok()?;

    let Some(next) = parts.next() else {
        return Some(ZedLink::Channel { channel_id });
    };

    if let Some(heading) = next.strip_prefix("notes#") {
        return Some(ZedLink::ChannelNotes {
            channel_id,
            heading: Some(heading.to_string()),
        });
    }

    if next == "notes" {
        return Some(ZedLink::ChannelNotes {
            channel_id,
            heading: None,
        });
    }

    None
}
