//! Discover/inspect only. Does not create or resume a session.
use crate::{
    bootstrap::discover,
    config::{AttachOptions, Limits},
    protocol::Inbound,
    transport::RpcClient,
};
use hermes_tern_core::error::{Error, Result};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub fn connect(options: &AttachOptions, limits: &Limits) -> Result<RpcClient> {
    RpcClient::connect(discover(options, limits)?, limits.clone())
}
/// A roster row is metadata only. Mint a new profile-bound grant and compare its
/// exact canonical home before opening any socket; never route via a sibling name.
pub fn connect_selected(
    options: &AttachOptions,
    limits: &Limits,
    selected: &Path,
) -> Result<RpcClient> {
    if options.home.as_deref() != Some(selected)
        || !selected.is_absolute()
        || !selected.is_dir()
        || selected.canonicalize()?.as_path() != selected
    {
        return Err(Error::Config(
            "selected profile must be an existing canonical home",
        ));
    }
    let grant = discover(options, limits)?;
    if Some(grant.profile_id.as_str()) != selected.to_str() {
        return Err(Error::Bootstrap("selected profile differs from grant"));
    }
    RpcClient::connect(grant, limits.clone())
}
/// For unbound inspection only. Session commands use the session event reducer.
pub fn call(
    client: &mut RpcClient,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value> {
    let id = client.request(method, params)?;
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(Error::Timeout("inspection"))?;
        match client.recv(remaining)? {
            Inbound::Response { id: got, result } if got == id => {
                return result.map_err(Error::Refused);
            }
            // This connection has not attached a session. Global broadcasters
            // can include a session_id; they are not responses to our read.
            Inbound::Event(_) => {}
            Inbound::ServerRequest { id, .. } => client.decline_server(&id)?,
            _ => return Err(Error::Protocol("unexpected inspection response")),
        }
    }
}
pub fn qualify(client: &mut RpcClient, limits: &Limits) -> Result<Value> {
    let pong = call(client, "ping", json!({}), limits.request_timeout)?;
    if pong["pong"] != true {
        return Err(Error::Protocol("ping failed"));
    }
    let describe = call(
        client,
        "runtime.describe",
        json!({}),
        limits.request_timeout,
    )?;
    if describe["instance_id"].as_str() != Some(client.instance_id.as_str())
        || describe["profile_id"].as_str() != Some(client.profile_id.as_str())
    {
        return Err(Error::Protocol("runtime identity differs from grant"));
    }
    let caps = describe["capabilities"]
        .as_array()
        .ok_or(Error::Unsupported("runtime capability report"))?;
    for required in [
        "durable-admission-v1",
        "event-replay-v1",
        "local-cli-create-v1",
    ] {
        if !caps.iter().any(|v| v == required) {
            return Err(Error::Unsupported(required));
        }
    }
    Ok(describe)
}
pub fn doctor(options: &AttachOptions, limits: &Limits) -> Result<Value> {
    let mut client = connect(options, limits)?;
    let describe = qualify(&mut client, limits)?;
    client.close();
    Ok(
        json!({"status":"connected", "protocol":"hermes-gateway-v1", "instance_id":client.instance_id, "profile_id":client.profile_id, "authority_epoch":describe["authority_epoch"].as_u64(), "durable_admission":true,"event_replay":true,"local_session_create":true,"session_mutation":false,"inference":false}),
    )
}
