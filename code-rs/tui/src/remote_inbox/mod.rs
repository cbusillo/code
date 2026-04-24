mod client;
pub(crate) mod protocol;

pub(crate) use client::RemoteInboxSession;
pub(crate) use client::RemoteInboxClientHandle;
pub(crate) use client::spawn_remote_inbox_client;
#[cfg(test)]
pub(crate) use client::test_remote_inbox_client_handle;
