//! Stable rooms and explicit user mention resolution. No participation inference.
use crate::{Error, Persona};

#[derive(Debug, Clone)]
pub struct Room {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub persona_id: Option<String>,
    pub members: Vec<RoomMember>,
    pub former_member_ids: Vec<String>,
}
#[derive(Debug, Clone)]
pub struct RoomMember {
    pub persona_id: String,
    pub membership_id: String,
    pub display_name: String,
}

pub(crate) fn invalid(detail: impl Into<String>) -> Error {
    Error::Room(detail.into())
}

pub(crate) fn mentions(
    prompt: &str,
    picked: &[String],
    members: &[RoomMember],
    personas: &[Persona],
) -> Result<(String, Vec<String>), Error> {
    let mut ids = Vec::new();
    let mut labels = Vec::new();
    for id in picked {
        let member = members
            .iter()
            .find(|m| &m.persona_id == id)
            .ok_or_else(|| invalid("Mentioned characters must be current group members."))?;
        if !ids.contains(id) {
            ids.push(id.clone());
            labels.push(format!("@{}", member.display_name));
        }
    }
    for (offset, _) in prompt.match_indices('@') {
        if prompt[..offset]
            .chars()
            .next_back()
            .is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | '[' | '{' | '“' | '「'))
        {
            continue;
        }
        let tail = &prompt[offset + 1..];
        let matches: Vec<_> = members
            .iter()
            .filter(|m| {
                tail.starts_with(&m.display_name)
                    && tail[m.display_name.len()..].chars().next().is_none_or(|c| {
                        c.is_whitespace()
                            || c.is_ascii_punctuation()
                            || matches!(
                                c,
                                '，' | '。' | '！' | '？' | '、' | '：' | '；' | '”' | '」'
                            )
                    })
            })
            .collect();
        let longest = matches.iter().map(|m| m.display_name.len()).max();
        let matches: Vec<_> = matches
            .into_iter()
            .filter(|m| Some(m.display_name.len()) == longest)
            .collect();
        if matches.len() != 1 {
            return Err(invalid(if matches.is_empty() {
                "Unknown @mention. Choose a current group member, or remove the @ token."
            } else {
                "Ambiguous @mention: characters share that name. Remove the typed token and choose the intended member explicitly."
            }));
        }
        let id = &matches[0].persona_id;
        if !personas.iter().any(|p| &p.id == id) {
            return Err(Error::UnknownPersona);
        }
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    let text = if labels.is_empty() {
        prompt.trim().to_owned()
    } else if prompt.trim().is_empty() {
        labels.join(" ")
    } else {
        format!("{}\n{}", labels.join(" "), prompt.trim())
    };
    Ok((text, ids))
}
