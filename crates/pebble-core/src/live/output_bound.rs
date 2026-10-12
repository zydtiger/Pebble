use super::failure;
use crate::Error;
use serde_json::Value;

const OUTPUT_CAP: usize = crate::memory::NATIVE_CAP;

#[derive(Default)]
pub(super) struct Bound {
    pub enabled: bool,
    bytes: usize,
}

impl Bound {
    pub fn add(&mut self, bytes: usize) -> Result<(), Error> {
        self.replace(0, bytes)
    }

    pub fn replace(&mut self, prior: usize, next: usize) -> Result<(), Error> {
        if !self.enabled {
            return Ok(());
        }
        let bytes = self.bytes.saturating_sub(prior).saturating_add(next);
        if bytes > OUTPUT_CAP {
            return Err(failure(
                "The model output exceeded the 128 KiB decoded output limit.",
            ));
        }
        self.bytes = bytes;
        Ok(())
    }
}

/// Count decoded strings/keys and scalar bytes, without serializing or escaping output.
/// Stop walking once the candidate is already oversized.
pub(super) fn decoded_bytes(value: &Value) -> usize {
    fn count(value: &Value, bytes: &mut usize) {
        if *bytes > OUTPUT_CAP {
            return;
        }
        match value {
            Value::String(text) => *bytes = bytes.saturating_add(text.len()),
            Value::Array(values) => {
                *bytes = bytes.saturating_add(2);
                for value in values {
                    count(value, bytes);
                }
            }
            Value::Object(fields) => {
                *bytes = bytes.saturating_add(2);
                for (key, value) in fields {
                    *bytes = bytes.saturating_add(key.len());
                    count(value, bytes);
                }
            }
            _ => *bytes = bytes.saturating_add(value.to_string().len()),
        }
    }
    let mut bytes = 0;
    count(value, &mut bytes);
    bytes
}
