use super::failure;
use crate::Error;

/// Decode complete UTF-8 lines after byte framing, independent of network fragments.
#[derive(Default)]
pub(super) struct Framing {
    pending: Vec<u8>,
    data: Vec<String>,
    frame_bytes: usize,
}
impl Framing {
    pub(super) fn feed(
        &mut self,
        bytes: &[u8],
        event: &mut impl FnMut(&str) -> Result<bool, Error>,
    ) -> Result<bool, Error> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let mut line = self.pending.drain(..=end).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.frame_bytes += line.len();
            if self.frame_bytes > 1024 * 1024 {
                return Err(failure("The model SSE event is too large."));
            }
            let line = std::str::from_utf8(&line)
                .map_err(|_| failure("The model stream contains invalid UTF-8."))?;
            if line.is_empty() {
                let data = self.data.join("\n");
                self.data.clear();
                self.frame_bytes = 0;
                if !data.is_empty() && event(&data)? {
                    return Ok(true);
                }
            } else if let Some(value) = line.strip_prefix("data:") {
                self.data
                    .push(value.strip_prefix(' ').unwrap_or(value).into());
            }
        }
        if self.pending.len() > 1024 * 1024 {
            return Err(failure("The model SSE line is too large."));
        }
        Ok(false)
    }
}
