//! Hermes canonical wire envelopes; generic JSON validation lives in core.
use hermes_tern_core::{
    error::{Error, Result, SafeReason},
    json::strict_json,
};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Event {
    pub kind: String,
    pub session_id: Option<String>,
    pub replay_epoch: Option<String>,
    pub seq: Option<u64>,
    pub authority_epoch: Option<u64>,
    pub generation: Option<u64>,
    pub admission_id: Option<String>,
    pub payload: Value,
}
#[derive(Debug)]
pub enum Inbound {
    Response {
        id: u64,
        result: std::result::Result<Value, SafeReason>,
    },
    Event(Event),
    ServerRequest {
        id: String,
        method: String,
        params: Value,
    },
    Disconnected,
}

pub fn parse_frame(raw: &str) -> Result<Inbound> {
    let v = strict_json(raw.as_bytes())?;
    if v["jsonrpc"] != "2.0" {
        return Err(Error::Protocol("wrong JSON-RPC version"));
    }
    if let Some(method) = v.get("method").and_then(Value::as_str) {
        if v.get("result").is_some() || v.get("error").is_some() {
            return Err(Error::Protocol("mixed envelope"));
        }
        let params = v
            .get("params")
            .filter(|v| v.is_object())
            .ok_or(Error::Protocol("params not object"))?
            .clone();
        if let Some(id) = v.get("id") {
            let id = id
                .as_str()
                .filter(|s| s.starts_with("srq-") && s.len() <= 160)
                .ok_or(Error::Protocol("invalid server request id"))?;
            return Ok(Inbound::ServerRequest {
                id: id.into(),
                method: method.into(),
                params,
            });
        }
        if method != "event" {
            return Err(Error::Protocol("unsupported notification envelope"));
        }
        let kind = params["type"]
            .as_str()
            .filter(|s| s.len() <= 128)
            .ok_or(Error::Protocol("missing event type"))?;
        return Ok(Inbound::Event(Event {
            kind: kind.into(),
            session_id: params["session_id"].as_str().map(str::to_owned),
            replay_epoch: params["replay_epoch"].as_str().map(str::to_owned),
            seq: params["seq"].as_u64(),
            authority_epoch: params["authority_epoch"].as_u64(),
            generation: params["execution_generation"].as_u64(),
            admission_id: params["admission_id"].as_str().map(str::to_owned),
            payload: params.get("payload").cloned().unwrap_or(Value::Null),
        }));
    }
    let id = v["id"]
        .as_u64()
        .ok_or(Error::Protocol("invalid response id"))?;
    let result = match (v.get("result"), v.get("error")) {
        (Some(r), None) => Ok(r.clone()),
        (None, Some(e)) if e.is_object() => Err(SafeReason::new(
            e["data"]["reason"]
                .as_str()
                .or_else(|| e["message"].as_str())
                .unwrap_or("request_failed"),
        )),
        _ => return Err(Error::Protocol("ambiguous response")),
    };
    Ok(Inbound::Response { id, result })
}
