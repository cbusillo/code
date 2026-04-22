mod client;
pub(crate) mod protocol;

pub(crate) use client::RemoteInboxSession;
pub(crate) use client::RemoteInboxClientHandle;
pub(crate) use client::spawn_remote_inbox_client;
