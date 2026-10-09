use super::failure;
use crate::Error;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    Id(String),
    Index(usize),
}
#[derive(Default)]
pub(super) struct Stream {
    visible: String,
    parts: BTreeMap<(Item, usize), String>,
    indexed_ids: BTreeMap<usize, String>,
    active_parts: BTreeMap<Item, usize>,
    next_parts: BTreeMap<Item, usize>,
    done_items: BTreeMap<String, Value>,
    output: Option<Vec<Value>>,
}
fn index(value: &Value) -> Result<usize, Error> {
    value
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| failure("The Responses stream has an invalid item index."))
}
impl Stream {
    fn item(&mut self, event: &Value) -> Result<Item, Error> {
        let id = event
            .get("item_id")
            .or_else(|| event.get("item").and_then(|item| item.get("id")));
        let position = event.get("output_index").map(index).transpose()?;
        if let Some(id) = id {
            let id = id
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| failure("The Responses stream has an invalid item ID."))?;
            if let Some(position) = position {
                if self
                    .indexed_ids
                    .get(&position)
                    .is_some_and(|prior| prior != id)
                {
                    return Err(failure("The Responses stream changed a message ID."));
                }
                self.indexed_ids.insert(position, id.into());
            }
            Ok(Item::Id(id.into()))
        } else if let Some(position) = position {
            Ok(Item::Index(position))
        } else {
            Err(failure("The Responses event has no item identity."))
        }
    }
    fn part(&self, event: &Value, item: &Item) -> Result<usize, Error> {
        if let Some(position) = event.get("content_index") {
            index(position)
        } else {
            self.active_parts
                .get(item)
                .copied()
                .ok_or_else(|| failure("The Responses text event has no active content part."))
        }
    }
    pub(super) fn event(
        &mut self,
        data: &str,
        delta: &mut impl FnMut(&str) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        let event: Value = serde_json::from_str(data)
            .map_err(|_| failure("The Responses stream contains malformed JSON."))?;
        let kind = event["type"]
            .as_str()
            .ok_or_else(|| failure("The Responses stream has no event type."))?;
        match kind {
            "error" | "response.failed" => {
                return Err(failure(
                    "The Responses endpoint reported a failed response.",
                ));
            }
            "response.incomplete" => {
                return Err(failure(
                    "The Responses output was incomplete or exhausted its output budget.",
                ));
            }
            "response.content_part.added" => {
                let item = self.item(&event)?;
                if self.active_parts.contains_key(&item) {
                    return Err(failure(
                        "The Responses stream has overlapping content parts.",
                    ));
                }
                let part = event
                    .get("content_index")
                    .map(index)
                    .transpose()?
                    .unwrap_or(*self.next_parts.get(&item).unwrap_or(&0));
                self.next_parts.insert(
                    item.clone(),
                    part.checked_add(1).ok_or_else(|| {
                        failure("The Responses stream has an invalid content index.")
                    })?,
                );
                self.active_parts.insert(item, part);
            }
            "response.content_part.done" => {
                let item = self.item(&event)?;
                let part = self.part(&event, &item)?;
                if self
                    .active_parts
                    .get(&item)
                    .is_some_and(|active| *active != part)
                {
                    return Err(failure(
                        "The Responses stream completed a different content part.",
                    ));
                }
                self.active_parts.remove(&item);
            }
            "response.output_text.delta" | "response.refusal.delta" => {
                let item = self.item(&event)?;
                let part = self.part(&event, &item)?;
                let text = event["delta"]
                    .as_str()
                    .ok_or_else(|| failure("The Responses stream has an invalid text delta."))?;
                self.parts.entry((item, part)).or_default().push_str(text);
                self.visible.push_str(text);
                if !text.is_empty() {
                    delta(text)?;
                }
            }
            "response.output_text.done" | "response.refusal.done" => {
                let item = self.item(&event)?;
                let part = self.part(&event, &item)?;
                let field = if kind == "response.refusal.done" {
                    "refusal"
                } else {
                    "text"
                };
                if self.parts.get(&(item, part)).map(String::as_str) != event[field].as_str() {
                    return Err(failure(
                        "The completed Responses text differs from the streamed reply.",
                    ));
                }
            }
            "response.output_item.added" => {
                supported(&event["item"])?;
                self.item(&event)?;
            }
            "response.output_item.done" => {
                supported(&event["item"])?;
                let Item::Id(id) = self.item(&event)? else {
                    return Err(failure("The completed Responses item has no ID."));
                };
                if self.done_items.insert(id, event["item"].clone()).is_some() {
                    return Err(failure(
                        "The Responses stream completed an item more than once.",
                    ));
                }
            }
            "response.completed" => {
                if event["response"]["status"] != "completed"
                    || event["response"].get("error").is_some_and(|e| !e.is_null())
                {
                    return Err(failure(
                        "The Responses endpoint did not complete successfully.",
                    ));
                }
                let items = event["response"]["output"]
                    .as_array()
                    .ok_or_else(|| failure("The completed Responses output is missing."))?;
                self.complete(items, delta)?;
                return Ok(true);
            }
            _ => {} // Lifecycle and reasoning events are never rendered.
        }
        Ok(false)
    }

    fn complete(
        &mut self,
        items: &[Value],
        delta: &mut impl FnMut(&str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut ids = HashSet::new();
        let mut visible = String::new();
        let mut final_parts = BTreeMap::new();
        let mut refusal_only = true;
        for (item_index, item) in items.iter().enumerate() {
            supported(item)?;
            let id = item["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| failure("The completed Responses item has no ID."))?;
            if !ids.insert(id) {
                return Err(failure(
                    "The completed Responses output has duplicate item IDs.",
                ));
            }
            if self
                .indexed_ids
                .get(&item_index)
                .is_some_and(|prior| prior != id)
            {
                return Err(failure(
                    "The completed Responses message ID differs from its stream.",
                ));
            }
            if item
                .get("status")
                .is_some_and(|status| status != "completed")
            {
                return Err(failure("A Responses output item did not complete."));
            }
            if item["type"] == "message" {
                if item["role"] != "assistant" || item["status"] != "completed" {
                    return Err(failure(
                        "The Responses output is not a completed assistant message.",
                    ));
                }
                let content = item["content"]
                    .as_array()
                    .ok_or_else(|| failure("The Responses message has invalid content."))?;
                for (content_index, part) in content.iter().enumerate() {
                    let text = match part["type"].as_str() {
                        Some("output_text") => {
                            refusal_only = false;
                            part["text"].as_str()
                        }
                        Some("refusal") => part["refusal"].as_str(),
                        _ => {
                            return Err(failure(
                                "The Responses message contains unsupported content.",
                            ));
                        }
                    }
                    .ok_or_else(|| failure("The Responses message has invalid text."))?;
                    visible.push_str(text);
                    final_parts.insert((Item::Id(id.into()), content_index), text.to_owned());
                    final_parts.insert((Item::Index(item_index), content_index), text.to_owned());
                }
            }
        }
        if self.indexed_ids.keys().any(|index| *index >= items.len())
            || self
                .done_items
                .iter()
                .any(|(id, done)| items.iter().find(|item| item["id"] == *id) != Some(done))
        {
            return Err(failure(
                "The completed Responses items differ from their final item events.",
            ));
        }
        if visible.trim().is_empty() {
            return Err(failure(
                "The Responses model completed without visible reply content.",
            ));
        }
        // Some providers supply refusal text only in the authoritative completion.
        if self.visible.is_empty() && refusal_only && self.parts.is_empty() {
            delta(&visible)?;
            self.visible = visible.clone();
        } else if self.visible != visible
            || self
                .parts
                .iter()
                .any(|(index, text)| final_parts.get(index) != Some(text))
        {
            return Err(failure(
                "The completed Responses text differs from the streamed reply.",
            ));
        }
        self.output = Some(items.to_vec());
        Ok(())
    }
    pub(super) fn output(self) -> Result<Vec<Value>, Error> {
        self.output
            .ok_or_else(|| failure("The Responses stream ended before completion."))
    }
}
fn supported(item: &Value) -> Result<(), Error> {
    if matches!(item["type"].as_str(), Some("reasoning" | "message")) {
        Ok(())
    } else {
        Err(failure(
            "The Responses model returned an unsupported output item; tools are unavailable.",
        ))
    }
}
