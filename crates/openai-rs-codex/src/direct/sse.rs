use super::DirectError;

pub(crate) enum SseItem {
    Data(String),
    Done,
}

pub(crate) struct SseDecoder {
    line: Vec<u8>,
    data: String,
    max_event_bytes: usize,
    done: bool,
}

impl SseDecoder {
    pub(crate) fn new(max_event_bytes: usize) -> Self {
        Self {
            line: Vec::new(),
            data: String::new(),
            max_event_bytes,
            done: false,
        }
    }

    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SseItem>, DirectError> {
        let mut output = Vec::new();
        for byte in chunk {
            if *byte == b'\n' {
                if self.line.last() == Some(&b'\r') {
                    self.line.pop();
                }
                self.process_line(&mut output)?;
                self.line.clear();
            } else {
                if self.line.len().saturating_add(self.data.len()) >= self.max_event_bytes {
                    return Err(DirectError::Sse("event exceeded size limit".to_owned()));
                }
                self.line.push(*byte);
            }
        }
        Ok(output)
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<SseItem>, DirectError> {
        let mut output = Vec::new();
        if !self.line.is_empty() {
            self.process_line(&mut output)?;
            self.line.clear();
        }
        if !self.data.is_empty() {
            self.dispatch(&mut output);
        }
        Ok(output)
    }

    fn process_line(&mut self, output: &mut Vec<SseItem>) -> Result<(), DirectError> {
        if self.line.is_empty() {
            self.dispatch(output);
            return Ok(());
        }
        if self.line.first() == Some(&b':') {
            return Ok(());
        }
        let line = std::str::from_utf8(&self.line)
            .map_err(|_| DirectError::Sse("event line was not UTF-8".to_owned()))?;
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        if field == "data" {
            let value = value.strip_prefix(' ').unwrap_or(value);
            if !self.data.is_empty() {
                self.data.push('\n');
            }
            self.data.push_str(value);
            if self.data.len() > self.max_event_bytes {
                return Err(DirectError::Sse("event exceeded size limit".to_owned()));
            }
        }
        Ok(())
    }

    fn dispatch(&mut self, output: &mut Vec<SseItem>) {
        if self.data.is_empty() || self.done {
            self.data.clear();
            return;
        }
        let data = std::mem::take(&mut self.data);
        if data.trim() == "[DONE]" {
            self.done = true;
            output.push(SseItem::Done);
        } else {
            output.push(SseItem::Data(data));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SseDecoder, SseItem};

    #[test]
    fn decodes_arbitrary_fragments_and_done() -> Result<(), super::DirectError> {
        let mut decoder = SseDecoder::new(1024);
        let mut items = Vec::new();
        for chunk in [
            &b"da"[..],
            &b"ta: {\"type\":\"response.created\"}\r\n\r"[..],
            &b"\ndata: [DONE]\n\n"[..],
        ] {
            items.extend(decoder.feed(chunk)?);
        }
        assert_eq!(items.len(), 2);
        assert!(matches!(items.first(), Some(SseItem::Data(_))));
        assert!(matches!(items.get(1), Some(SseItem::Done)));
        Ok(())
    }

    #[test]
    fn rejects_oversize_event() {
        let mut decoder = SseDecoder::new(4);
        assert!(decoder.feed(b"data: too long").is_err());
    }
}
