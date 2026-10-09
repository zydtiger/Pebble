use crate::Error;
use std::io::{Cursor, Read};

pub(super) const DECODED_CAP: usize = 8 * 1024 * 1024;
pub(super) const ITEM_OVERHEAD: usize = 64;
fn corrupt() -> Error {
    Error::History("A model history item has an invalid codec, length, or compressed payload. The saved transcript is intact; start a fresh context.".into())
}
pub(super) fn encode(bytes: Vec<u8>) -> Result<(&'static str, Vec<u8>, usize), Error> {
    let length = bytes.len();
    if length >= 1024 {
        let compressed = zstd::stream::encode_all(Cursor::new(&bytes), 3).map_err(|_| corrupt())?;
        if compressed.len() < length {
            return Ok(("zstd", compressed, length));
        }
    }
    Ok(("json", bytes, length))
}
pub(super) fn decode(
    codec: &str,
    payload: Vec<u8>,
    length: usize,
    remaining: usize,
) -> Result<Vec<u8>, Error> {
    if length > remaining || payload.len() > DECODED_CAP {
        return Err(cap_error());
    }
    let bytes = match codec {
        "json" => payload,
        "zstd" => {
            let mut decoder =
                zstd::stream::read::Decoder::new(Cursor::new(payload)).map_err(|_| corrupt())?;
            // Bound decoder's own window independently of the untrusted declared length.
            decoder.window_log_max(23).map_err(|_| corrupt())?;
            let mut bytes = Vec::new();
            decoder
                .take((length.min(remaining) + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| corrupt())?;
            bytes
        }
        _ => return Err(corrupt()),
    };
    if bytes.len() != length {
        return Err(corrupt());
    }
    Ok(bytes)
}
pub(super) fn metadata(codec: &str, length: i64, stored: i64) -> Result<usize, Error> {
    if !matches!(codec, "json" | "zstd") || length < 0 || stored < 0 {
        return Err(corrupt());
    }
    if length > DECODED_CAP as i64 || stored > DECODED_CAP as i64 {
        return Err(cap_error());
    }
    if codec == "json" && length != stored {
        return Err(corrupt());
    }
    Ok(length as usize)
}
pub(super) fn cap_error() -> Error {
    Error::ContextLimit("Model history exceeds the 8 MiB decoded context limit. Start a fresh context; earlier messages stay saved. Increasing token limits cannot raise this memory bound.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_and_compressed_roundtrip_exact_bytes() {
        for bytes in [
            b"small native item".to_vec(),
            "你好 🪨 opaque-state ".repeat(1000).into_bytes(),
        ] {
            let original = bytes.clone();
            let (codec, stored, length) = encode(bytes).unwrap();
            assert_eq!(codec, if length < 1024 { "json" } else { "zstd" });
            assert_eq!(
                decode(codec, stored, length, DECODED_CAP).unwrap(),
                original
            );
        }
        let mut state = 42u64;
        let random: Vec<u8> = (0..4096)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        let (codec, bytes, _) = encode(random.clone()).unwrap();
        assert_eq!(codec, "json");
        assert_eq!(bytes, random);
    }
    #[test]
    fn corrupt_and_lying_lengths_are_bounded() {
        let bytes = vec![b'x'; DECODED_CAP + 1];
        let compressed = zstd::stream::encode_all(Cursor::new(bytes), 3).unwrap();
        assert!(decode("zstd", compressed.clone(), 1, DECODED_CAP).is_err());
        assert!(decode("zstd", compressed, DECODED_CAP + 1, DECODED_CAP).is_err());
        let (_, mut stored, length) = encode(vec![b'x'; 4000]).unwrap();
        stored.pop();
        assert!(decode("zstd", stored, length, DECODED_CAP).is_err());
        assert!(decode("unknown", vec![], 0, DECODED_CAP).is_err());
        assert!(decode("json", vec![1], 2, DECODED_CAP).is_err());
    }
}
