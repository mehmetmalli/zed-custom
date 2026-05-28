//! Minimal stubs of the pre-removal `client::User` / `UserStore` types. The
//! Zed sign-in flow has been removed, so these never reflect an authenticated
//! user. They exist so call sites that took an `Entity<UserStore>` keep
//! compiling.

use super::Client;
use anyhow::Result;
use chrono::{DateTime, Utc};
use cloud_api_types::{OrganizationConfiguration, OrganizationId, Plan};
use collections::HashMap;
use derive_more::Deref;
use gpui::{App, AsyncApp, Context, EventEmitter, SharedString, SharedUri, Task};
use postage::watch;
use rpc::proto;
use std::sync::Arc;
use text::ReplicaId;

pub type LegacyUserId = u64;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct ProjectId(pub u64);

impl ProjectId {
    pub fn to_proto(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticipantIndex(pub u32);

#[derive(Default, Debug)]
pub struct User {
    pub legacy_id: LegacyUserId,
    pub github_login: SharedString,
    pub avatar_uri: SharedUri,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collaborator {
    pub peer_id: proto::PeerId,
    pub replica_id: ReplicaId,
    pub user_id: LegacyUserId,
    pub is_host: bool,
    pub committer_name: Option<String>,
    pub committer_email: Option<String>,
}

impl PartialOrd for User {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for User {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.github_login.cmp(&other.github_login)
    }
}

impl PartialEq for User {
    fn eq(&self, other: &Self) -> bool {
        self.legacy_id == other.legacy_id && self.github_login == other.github_login
    }
}

impl Eq for User {}

#[derive(Debug, Clone, Copy)]
pub struct RequestUsage {
    pub limit: cloud_llm_client::UsageLimit,
    pub amount: i32,
}

#[derive(Debug, Clone, Copy, Deref)]
pub struct EditPredictionUsage(pub RequestUsage);

impl RequestUsage {
    pub fn over_limit(&self) -> bool {
        match self.limit {
            cloud_llm_client::UsageLimit::Limited(limit) => self.amount >= limit,
            cloud_llm_client::UsageLimit::Unlimited => false,
        }
    }
}

impl EditPredictionUsage {
    /// Parses usage information from response headers. After the Zed cloud
    /// LLM provider was removed nothing actually emits these headers, but
    /// the helper is kept so external crates that handle generic HTTP
    /// responses still compile.
    pub fn from_headers(
        _headers: &http_client::http::HeaderMap<http_client::http::HeaderValue>,
    ) -> Result<Self> {
        anyhow::bail!("edit prediction usage headers are no longer reported");
    }
}

pub struct UserStore {
    current_user: watch::Receiver<Option<Arc<User>>>,
    participant_indices: HashMap<u64, ParticipantIndex>,
    users: HashMap<u64, Arc<User>>,
}

pub enum Event {
    PrivateUserInfoUpdated,
    PlanUpdated,
    OrganizationChanged,
    ParticipantIndicesChanged,
}

impl EventEmitter<Event> for UserStore {}

impl UserStore {
    pub fn new(_client: Arc<Client>, _cx: &Context<Self>) -> Self {
        let (_tx, rx) = watch::channel::<Option<Arc<User>>>();
        Self {
            current_user: rx,
            participant_indices: HashMap::default(),
            users: HashMap::default(),
        }
    }

    pub fn watch_current_user(&self) -> watch::Receiver<Option<Arc<User>>> {
        self.current_user.clone()
    }

    pub fn current_user(&self) -> Option<Arc<User>> {
        self.current_user.borrow().clone()
    }

    pub fn current_organization(&self) -> Option<Arc<Organization>> {
        None
    }

    pub fn current_organization_configuration(&self) -> Option<&OrganizationConfiguration> {
        None
    }

    pub fn organizations(&self) -> &[Arc<Organization>] {
        &[]
    }

    pub fn plan(&self) -> Option<Plan> {
        None
    }

    pub fn plan_for_organization(&self, _organization_id: &OrganizationId) -> Option<Plan> {
        None
    }

    pub fn subscription_period(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        None
    }

    pub fn trial_started_at(&self) -> Option<DateTime<Utc>> {
        None
    }

    pub fn account_too_young(&self) -> bool {
        false
    }

    pub fn has_overdue_invoices(&self) -> bool {
        false
    }

    pub fn edit_prediction_usage(&self) -> Option<EditPredictionUsage> {
        None
    }

    pub fn update_edit_prediction_usage(
        &mut self,
        _usage: EditPredictionUsage,
        _cx: &mut Context<Self>,
    ) {
    }

    pub fn get_cached_user(&self, user_id: u64) -> Option<Arc<User>> {
        self.users.get(&user_id).cloned()
    }

    pub fn cached_user_by_github_login(&self, _github_login: &str) -> Option<Arc<User>> {
        None
    }

    pub fn get_users(
        &self,
        _user_ids: Vec<u64>,
        _cx: &Context<Self>,
    ) -> Task<Result<Vec<Arc<User>>>> {
        Task::ready(Ok(Vec::new()))
    }

    pub fn get_user(&self, _user_id: u64, _cx: &Context<Self>) -> Task<Result<Arc<User>>> {
        Task::ready(Err(anyhow::anyhow!(
            "user lookup unavailable: zed.dev sign-in has been removed"
        )))
    }

    pub fn get_user_optimistic(&self, user_id: u64, _cx: &Context<Self>) -> Option<Arc<User>> {
        self.users.get(&user_id).cloned()
    }

    pub fn fuzzy_search_users(
        &self,
        _query: String,
        _cx: &Context<Self>,
    ) -> Task<Result<Vec<Arc<User>>>> {
        Task::ready(Ok(Vec::new()))
    }

    pub fn set_participant_indices(
        &mut self,
        participant_indices: HashMap<u64, ParticipantIndex>,
        cx: &mut Context<Self>,
    ) {
        if participant_indices != self.participant_indices {
            self.participant_indices = participant_indices;
            cx.emit(Event::ParticipantIndicesChanged);
        }
    }

    pub fn participant_indices(&self) -> &HashMap<u64, ParticipantIndex> {
        &self.participant_indices
    }

    pub fn participant_names(
        &self,
        user_ids: impl Iterator<Item = u64>,
        _cx: &App,
    ) -> HashMap<u64, SharedString> {
        let mut ret = HashMap::default();
        for id in user_ids {
            if let Some(user) = self.users.get(&id) {
                ret.insert(id, user.github_login.clone());
            }
        }
        ret
    }

    pub fn insert(&mut self, users: Vec<proto::User>) -> Vec<Arc<User>> {
        let mut ret = Vec::with_capacity(users.len());
        for user in users {
            let user = User::from_proto(user);
            self.users.insert(user.legacy_id, user.clone());
            ret.push(user);
        }
        ret
    }

    pub fn set_current_organization(
        &mut self,
        _organization: Arc<Organization>,
        _cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn clear_organizations(&mut self) {}

    pub fn clear_plan_and_usage(&mut self) {}

    pub fn clear_contacts(&self) -> impl std::future::Future<Output = ()> + use<> {
        async {}
    }

    pub fn contact_updates_done(&self) -> impl std::future::Future<Output = ()> + use<> {
        async {}
    }

    pub fn has_contact(&self, _user: &Arc<User>) -> bool {
        false
    }

    pub fn is_contact_request_pending(&self, _user: &User) -> bool {
        false
    }

    pub fn contact_request_status(&self, _user: &User) -> ContactRequestStatus {
        ContactRequestStatus::None
    }

    pub fn contacts(&self) -> &[Arc<Contact>] {
        &[]
    }

    pub fn incoming_contact_requests(&self) -> &[Arc<User>] {
        &[]
    }

    pub fn outgoing_contact_requests(&self) -> &[Arc<User>] {
        &[]
    }

    pub fn request_contact(
        &mut self,
        _responder_id: u64,
        _cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn remove_contact(
        &mut self,
        _user_id: u64,
        _cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn respond_to_contact_request(
        &mut self,
        _requester_id: u64,
        _accept: bool,
        _cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn dismiss_contact_request(
        &self,
        _requester_id: u64,
        _cx: &Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn has_incoming_contact_request(&self, _user_id: u64) -> bool {
        false
    }

    #[cfg(feature = "test-support")]
    pub fn clear_cache(&mut self) {
        self.users.clear();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_current_organization_configuration_for_test(
        &mut self,
        _organization: Arc<Organization>,
        _configuration: OrganizationConfiguration,
        _cx: &mut Context<Self>,
    ) {
    }
}

#[derive(Debug, Clone)]
pub struct Organization {
    pub id: OrganizationId,
    pub name: SharedString,
    pub is_personal: bool,
}

#[derive(Debug, PartialEq)]
pub struct Contact {
    pub user: Arc<User>,
    pub online: bool,
    pub busy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactRequestStatus {
    None,
    RequestSent,
    RequestReceived,
    RequestAccepted,
}

#[derive(Clone, Copy)]
pub enum ContactEventKind {
    Requested,
    Accepted,
    Cancelled,
}

impl User {
    fn from_proto(message: proto::User) -> Arc<Self> {
        Arc::new(User {
            legacy_id: message.id,
            github_login: message.github_login.into(),
            avatar_uri: message.avatar_url.into(),
            name: message.name,
        })
    }
}

impl Collaborator {
    pub fn from_proto(message: proto::Collaborator) -> Result<Self> {
        use anyhow::Context as _;
        Ok(Self {
            peer_id: message.peer_id.context("invalid peer id")?,
            replica_id: ReplicaId::new(message.replica_id as u16),
            user_id: message.user_id as LegacyUserId,
            is_host: message.is_host,
            committer_name: message.committer_name,
            committer_email: message.committer_email,
        })
    }
}

#[allow(unused_variables)]
pub fn _unused(_: &AsyncApp) {}
