//! The Teams board's derivation: the coordination the mesh already
//! carries in its message kinds, extracted from the desktop's own
//! realm-scoped stores (joined rooms + lobby help broadcasts). Ported
//! from lazymesh's team.go, and pure so every derivation is testable
//! without a live mesh.

use serde::Serialize;

use crate::mesh::{JoinedRoom, MeshEvent};

/// TeamItem is one row of team activity: who did something, what it
/// was, in which room.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamItem {
    pub who: String,
    pub petname: String,
    pub text: String,
    pub room: String,
}

/// TeamBoard is everything the Teams tab renders.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamBoard {
    pub open_lanes: Vec<TeamItem>,
    pub open_handoffs: Vec<TeamItem>,
    pub recent_results: Vec<TeamItem>,
    pub help: Vec<TeamItem>,
}

/// board derives the full team board from the joined rooms and the
/// lobby's help broadcasts.
pub fn board(joined: &[JoinedRoom], help: &[MeshEvent]) -> TeamBoard {
    TeamBoard {
        open_lanes: open_pairs(joined, "lane_claimed", "lane_released"),
        open_handoffs: open_pairs(joined, "task_handed_over", "result_reported"),
        recent_results: recent_results(joined, 5),
        help: help_broadcasts(help, 5),
    }
}

/// open_pairs is the shared derivation: every `claim`-kind message
/// whose message_id is not referenced as the in_reply_to of a
/// `release`-kind message counts as still open.
fn open_pairs(joined: &[JoinedRoom], claim: &str, release: &str) -> Vec<TeamItem> {
    let mut released = Vec::new();
    let mut candidates: Vec<(TeamItem, String)> = Vec::new();
    for room in joined {
        for message in &room.messages {
            let Some(envelope) = envelope(message) else {
                continue;
            };
            if envelope.kind == release {
                released.extend(envelope.in_reply_to.clone());
            }
            if envelope.kind == claim {
                let item = envelope.item(message);
                candidates.push((item, envelope.message_id));
            }
        }
    }
    candidates
        .into_iter()
        .filter(|(_, id)| !released.contains(id))
        .map(|(item, _)| item)
        .collect()
}

/// recent_results collects the newest result_reported messages, newest
/// last, capped at n.
fn recent_results(joined: &[JoinedRoom], n: usize) -> Vec<TeamItem> {
    let mut items: Vec<TeamItem> = joined
        .iter()
        .flat_map(|room| room.messages.iter())
        .filter_map(|message| {
            let envelope = envelope(message)?;
            (envelope.kind == "result_reported").then(|| envelope.item(message))
        })
        .collect();
    let keep = items.len().saturating_sub(n);
    items = items[keep..].to_vec();
    items
}

/// help_broadcasts picks the newest help_requested/help_offered
/// broadcasts off the lobby, newest last, capped at n.
fn help_broadcasts(help: &[MeshEvent], n: usize) -> Vec<TeamItem> {
    let mut items: Vec<TeamItem> = help
        .iter()
        .filter_map(|message| {
            let envelope = envelope(message)?;
            (envelope.kind == "help_requested" || envelope.kind == "help_offered")
                .then(|| envelope.item(message))
        })
        .collect();
    let keep = items.len().saturating_sub(n);
    items = items[keep..].to_vec();
    items
}

/// Envelope is the subset of a room message's payload the board needs.
struct Envelope {
    kind: String,
    message_id: String,
    in_reply_to: Option<String>,
    text: String,
}

impl Envelope {
    fn item(&self, message: &MeshEvent) -> TeamItem {
        let who = message.publisher.clone();
        TeamItem {
            petname: crate::petname::petname(&who),
            who,
            text: self.text.clone(),
            room: short_topic(&message.topic),
        }
    }
}

fn envelope(message: &MeshEvent) -> Option<Envelope> {
    let payload: serde_json::Value = serde_json::from_str(&message.payload).ok()?;
    let kind = payload.get("kind")?.as_str()?.to_string();
    Some(Envelope {
        message_id: payload
            .get("message_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        in_reply_to: payload
            .get("in_reply_to")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        text: payload
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        kind,
    })
}

fn short_topic(topic: &str) -> String {
    topic
        .strip_prefix("agents.room.")
        .unwrap_or(topic)
        .chars()
        .take(8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{JoinedRoom, MeshEvent};

    fn room_msg(topic: &str, payload: &str, seq: u64) -> MeshEvent {
        MeshEvent {
            topic: topic.to_string(),
            payload: payload.to_string(),
            publisher: "a".repeat(64),
            seq,
        }
    }

    #[test]
    fn lane_claimed_stays_open_until_released() {
        let joined = vec![JoinedRoom {
            topic: "agents.room.11111111".to_string(),
            purpose: String::new(),
            messages: vec![
                room_msg(
                    "agents.room.11111111",
                    r#"{"kind":"lane_claimed","message_id":"c1","text":"taking the editor"}"#,
                    1,
                ),
                room_msg(
                    "agents.room.11111111",
                    r#"{"kind":"lane_released","message_id":"r1","in_reply_to":"c1","text":""}"#,
                    2,
                ),
                room_msg(
                    "agents.room.11111111",
                    r#"{"kind":"lane_claimed","message_id":"c2","text":"taking termkeys"}"#,
                    3,
                ),
            ],
        }];
        let board = board(&joined, &[]);
        assert_eq!(board.open_lanes.len(), 1);
        assert_eq!(board.open_lanes[0].text, "taking termkeys");
    }

    #[test]
    fn help_broadcasts_filter_kinds() {
        let help = vec![
            room_msg(
                "agents.lobby",
                r#"{"kind":"remark_made","text":"noise"}"#,
                1,
            ),
            room_msg(
                "agents.lobby",
                r#"{"kind":"help_requested","text":"need a review"}"#,
                2,
            ),
        ];
        let board = board(&[], &help);
        assert_eq!(board.help.len(), 1);
        assert_eq!(board.help[0].text, "need a review");
    }
}
