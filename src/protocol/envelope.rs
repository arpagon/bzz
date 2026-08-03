use nostr::{Event, JsonUtil as _};
use serde_json::{Value, json};

use crate::error::{Error, Result};

#[derive(Clone, Debug)]
pub enum RelayMessage {
    Auth(String),
    Event {
        subscription: String,
        event: Event,
    },
    Eose(String),
    Ok {
        event_id: String,
        accepted: bool,
        message: String,
    },
    Notice(String),
    Closed {
        subscription: String,
        message: String,
    },
    Count {
        subscription: String,
        count: u64,
    },
    Unknown(Value),
}

impl RelayMessage {
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > 8 * 1024 * 1024 {
            return Err(Error::Protocol(
                "relay envelope exceeds the size limit".into(),
            ));
        }
        let value: Value = serde_json::from_str(text)
            .map_err(|error| Error::Protocol(format!("invalid relay JSON: {error}")))?;
        let array = value
            .as_array()
            .ok_or_else(|| Error::Protocol("relay envelope is not an array".into()))?;
        let kind = array.first().and_then(Value::as_str).unwrap_or_default();
        let string = |index: usize| {
            array
                .get(index)
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Protocol(format!("malformed {kind} envelope")))
        };
        match kind {
            "AUTH" if array.len() == 2 => Ok(Self::Auth(string(1)?.to_owned())),
            "EVENT" if array.len() == 3 => {
                let subscription = string(1)?.to_owned();
                let event = Event::from_json(array[2].to_string())
                    .map_err(|error| Error::Protocol(format!("invalid event: {error}")))?;
                Ok(Self::Event {
                    subscription,
                    event,
                })
            }
            "EOSE" if array.len() == 2 => Ok(Self::Eose(string(1)?.to_owned())),
            "OK" if array.len() >= 4 => {
                let message = string(3)?;
                if message.len() > 8_192 {
                    return Err(Error::Protocol("OK message exceeds the size limit".into()));
                }
                Ok(Self::Ok {
                    event_id: string(1)?.to_owned(),
                    accepted: array[2]
                        .as_bool()
                        .ok_or_else(|| Error::Protocol("malformed OK envelope".into()))?,
                    message: message.to_owned(),
                })
            }
            "NOTICE" if array.len() == 2 => Ok(Self::Notice(string(1)?.to_owned())),
            "CLOSED" if array.len() >= 3 => Ok(Self::Closed {
                subscription: string(1)?.to_owned(),
                message: string(2)?.to_owned(),
            }),
            "COUNT" if array.len() >= 3 => Ok(Self::Count {
                subscription: string(1)?.to_owned(),
                count: array[2].get("count").and_then(Value::as_u64).unwrap_or(0),
            }),
            _ => Ok(Self::Unknown(value)),
        }
    }
}

pub fn auth(event: &Event) -> String {
    json!(["AUTH", event]).to_string()
}

pub fn publish(event: &Event) -> String {
    json!(["EVENT", event]).to_string()
}

pub fn request(subscription: &str, filters: &[Value]) -> Result<String> {
    if subscription.is_empty() || subscription.len() > 64 {
        return Err(Error::Protocol("invalid subscription ID".into()));
    }
    let mut envelope = vec![json!("REQ"), json!(subscription)];
    envelope.extend(filters.iter().cloned());
    Ok(Value::Array(envelope).to_string())
}

pub fn close(subscription: &str) -> String {
    json!(["CLOSE", subscription]).to_string()
}
