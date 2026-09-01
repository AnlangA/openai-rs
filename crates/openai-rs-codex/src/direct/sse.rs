//! Private SSE byte-stream decoder for the sealed direct Codex transport.
//!
//! Framing follows WHATWG `text/event-stream` and stays aligned with the
//! platform decoder in `openai-rs-client::sse` on the edges that matter for
//! Codex streams:
//!
//! - a lone CR terminates a line exactly like LF and CRLF does, and a CRLF
//!   pair split across two chunks is one terminator, not two;
//! - a leading UTF-8 BOM is stripped from the stream start only;
//! - `data: [DONE]` matches the sentinel exactly (no surrounding whitespace),
//!   mirroring the platform `SseEndpointPolicy` sentinel comparison;
//! - the physical-line limit and the joined-event limit are enforced
//!   separately, each failing only when a completed length strictly exceeds
//!   its limit (a line or event exactly at the limit is accepted);
//! - decoding is fail-stop: after an error the caller must not feed again.

use super::DirectError;

/// Byte prefix that a UTF-8 BOM must match at the stream start.
const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

pub(crate) enum SseItem {
    Data(String),
    Done,
}

pub(crate) struct SseDecoder {
    /// Accumulated bytes of the current physical line, without its terminator.
    line: Vec<u8>,
    /// Joined `data:` values of the event currently being assembled.
    data: String,
    /// Whether the current event saw any `data:` field, including an explicit
    /// empty one (`data:`). The joined string alone cannot express that.
    has_data: bool,
    max_line_bytes: usize,
    max_event_bytes: usize,
    /// Whether the BOM decision (stream start) has been made yet.
    bom_checked: bool,
    /// Length of a still-undecided strict BOM prefix buffered at stream start.
    bom_len: usize,
    /// Whether the previous byte was a CR that already terminated its line,
    /// so a following LF is the second half of a CRLF pair.
    after_cr: bool,
    done: bool,
}

impl SseDecoder {
    pub(crate) fn new(max_line_bytes: usize, max_event_bytes: usize) -> Self {
        Self {
            line: Vec::new(),
            data: String::new(),
            has_data: false,
            max_line_bytes,
            max_event_bytes,
            bom_checked: false,
            bom_len: 0,
            after_cr: false,
            done: false,
        }
    }

    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SseItem>, DirectError> {
        let mut output = Vec::new();
        for &byte in chunk {
            if !self.bom_checked {
                if byte == UTF8_BOM[self.bom_len] {
                    self.bom_len += 1;
                    if self.bom_len == UTF8_BOM.len() {
                        // A complete BOM at the stream start is framing and is
                        // discarded without ever counting against the line
                        // limit.
                        self.bom_checked = true;
                    }
                    continue;
                }
                // Not a BOM after all: the buffered prefix was ordinary line
                // content and is replayed before the mismatching byte.
                self.bom_checked = true;
                let replay = self.bom_len;
                for &prefix in &UTF8_BOM[..replay] {
                    self.push_line_byte(&mut output, prefix)?;
                }
            }
            self.push_line_byte(&mut output, byte)?;
        }
        Ok(output)
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<SseItem>, DirectError> {
        let mut output = Vec::new();
        if !self.bom_checked {
            // A stream that ended inside a strict BOM prefix: those bytes were
            // content, not framing.
            self.bom_checked = true;
            let replay = self.bom_len;
            for &prefix in &UTF8_BOM[..replay] {
                self.push_line_byte(&mut output, prefix)?;
            }
        }
        if !self.line.is_empty() {
            let line = std::mem::take(&mut self.line);
            self.process_line(&line, &mut output)?;
        }
        if self.has_data {
            self.dispatch(&mut output);
        }
        Ok(output)
    }

    /// Advance the WHATWG tokenizer by one content byte.
    fn push_line_byte(&mut self, output: &mut Vec<SseItem>, byte: u8) -> Result<(), DirectError> {
        match byte {
            b'\r' => {
                self.after_cr = true;
                self.terminate_line(output)?;
            }
            b'\n' => {
                // The LF of a CRLF pair terminates nothing: the CR already
                // ended the line, and a second empty line here would dispatch
                // the event twice.
                if !self.after_cr {
                    self.terminate_line(output)?;
                }
                self.after_cr = false;
            }
            _ => {
                self.after_cr = false;
                if self.line.len() >= self.max_line_bytes {
                    return Err(DirectError::Sse(format!(
                        "line exceeded the {}-byte limit",
                        self.max_line_bytes
                    )));
                }
                self.line.push(byte);
            }
        }
        Ok(())
    }

    fn terminate_line(&mut self, output: &mut Vec<SseItem>) -> Result<(), DirectError> {
        let line = std::mem::take(&mut self.line);
        self.process_line(&line, output)
    }

    fn process_line(
        &mut self,
        line_bytes: &[u8],
        output: &mut Vec<SseItem>,
    ) -> Result<(), DirectError> {
        if line_bytes.is_empty() {
            self.dispatch(output);
            return Ok(());
        }
        if line_bytes.first() == Some(&b':') {
            return Ok(());
        }
        let line = std::str::from_utf8(line_bytes)
            .map_err(|_| DirectError::Sse("event line was not UTF-8".to_owned()))?;
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        if field == "data" {
            let value = value.strip_prefix(' ').unwrap_or(value);
            let separator_bytes = usize::from(self.has_data);
            let next_len = self
                .data
                .len()
                .checked_add(separator_bytes)
                .and_then(|len| len.checked_add(value.len()))
                .ok_or_else(|| DirectError::Sse("event exceeded size limit".to_owned()))?;
            if next_len > self.max_event_bytes {
                return Err(DirectError::Sse("event exceeded size limit".to_owned()));
            }
            if self.has_data {
                self.data.push('\n');
            }
            self.data.push_str(value);
            self.has_data = true;
        }
        Ok(())
    }

    fn dispatch(&mut self, output: &mut Vec<SseItem>) {
        if self.done {
            self.data.clear();
            self.has_data = false;
            return;
        }
        if !self.has_data {
            return;
        }
        let data = std::mem::take(&mut self.data);
        self.has_data = false;
        if data == "[DONE]" {
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

    fn data_items(items: Vec<SseItem>) -> Vec<String> {
        items
            .into_iter()
            .filter_map(|item| match item {
                SseItem::Data(data) => Some(data),
                SseItem::Done => None,
            })
            .collect()
    }

    #[test]
    fn decodes_arbitrary_fragments_and_done() -> Result<(), super::DirectError> {
        let mut decoder = SseDecoder::new(1024, 1024);
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
    fn decodes_one_byte_at_a_time() -> Result<(), super::DirectError> {
        // Every terminator style (CR, LF, CRLF) appears, including a CRLF pair
        // split across iterations.
        let input = b"data: first\r\rdata: second\r\n\r\ndata: third\n\n";
        let mut decoder = SseDecoder::new(1024, 1024);
        let mut items = Vec::new();
        for byte in input {
            items.extend(decoder.feed(&[*byte])?);
        }
        items.extend(decoder.finish()?);
        assert_eq!(data_items(items), vec!["first", "second", "third"]);
        Ok(())
    }

    /// 7-07(a): WHATWG treats a lone CR as a line terminator. A CR ends the
    /// line, a following LF is the tail of one CRLF (never a second blank
    /// line), and two CRs form an empty line that dispatches the event.
    #[test]
    fn lone_cr_terminates_lines_and_split_crlf_is_one_terminator() -> Result<(), super::DirectError>
    {
        let mut decoder = SseDecoder::new(1024, 1024);
        let mut items = decoder.feed(b"data: one\rdata: two\r\rdata: three\n\n")?;
        items.extend(decoder.finish()?);
        // `one` and `two` are data fields of one event joined by a newline;
        // only the blank CR CR line dispatches it.
        assert_eq!(data_items(items), vec!["one\ntwo", "three"]);

        let mut split = SseDecoder::new(1024, 1024);
        let mut items = split.feed(b"data: x\r")?;
        assert!(items.is_empty());
        items.extend(split.feed(b"\n\r")?);
        assert_eq!(data_items(items), vec!["x"]);
        let mut later = split.feed(b"data: y\r\r")?;
        later.extend(split.finish()?);
        assert_eq!(data_items(later), vec!["y"]);
        Ok(())
    }

    /// 7-07(b): a leading UTF-8 BOM is framing and is stripped only at the
    /// stream start, even when split across chunks; mid-stream U+FEFF bytes
    /// stay payload content.
    #[test]
    fn strips_a_leading_utf8_bom_only_at_stream_start() -> Result<(), super::DirectError> {
        let mut decoder = SseDecoder::new(1024, 1024);
        let mut items = decoder.feed(&[0xEF])?;
        assert!(items.is_empty());
        items.extend(decoder.feed(&[0xBB, 0xBF])?);
        items.extend(decoder.feed(b"data: first\n\n")?);
        assert_eq!(data_items(items), vec!["first"]);

        let mut later = SseDecoder::new(1024, 1024);
        let mut payload = b"data: ".to_vec();
        payload.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        payload.extend_from_slice(b"second\n\n");
        let items = later.feed(&payload)?;
        assert_eq!(data_items(items), vec!["\u{feff}second"]);
        Ok(())
    }

    /// A strict BOM prefix followed by a non-BOM byte is content: the replayed
    /// bytes reach the line decoder and fail its UTF-8 validation.
    #[test]
    fn a_bom_prefix_turned_content_still_validates_utf8() {
        let mut decoder = SseDecoder::new(1024, 1024);
        assert!(decoder.feed(&[0xEF, 0xBB]).is_ok());
        let error = decoder.feed(b"\ndata: x\n\n").err();
        assert!(error.is_some_and(|error| matches!(error, super::DirectError::Sse(_))));
    }

    /// 7-07(c): the terminal sentinel matches the joined `data` value exactly,
    /// like the platform `SseEndpointPolicy` sentinel comparison. Surrounding
    /// whitespace keeps it an ordinary (and, for this endpoint, undecodable)
    /// data frame instead of a terminator.
    #[test]
    fn done_marker_matches_exactly_without_trimming() -> Result<(), super::DirectError> {
        let mut decoder = SseDecoder::new(1024, 1024);
        let items = decoder.feed(b"data: [DONE]\n\n")?;
        assert_eq!(items.len(), 1);
        assert!(matches!(items.first(), Some(SseItem::Done)));

        let mut padded = SseDecoder::new(1024, 1024);
        let items = padded.feed(b"data:  [DONE] \n\n")?;
        assert_eq!(data_items(items), vec![" [DONE] "]);
        Ok(())
    }

    /// 7-07(e): the line and event limits are separate, and each uses the
    /// platform decoder's `>` judgment — a line or event exactly at the limit
    /// is accepted, one byte more fails.
    #[test]
    fn enforces_line_and_event_limits_separately() -> Result<(), super::DirectError> {
        let mut line_decoder = SseDecoder::new(9, 1024);
        assert!(line_decoder.feed(b"data: ok!\n\n").is_ok());
        assert!(line_decoder.feed(b"data: too long\n\n").is_err());

        let mut event_decoder = SseDecoder::new(1024, 6);
        assert!(event_decoder.feed(b"data: abcdef\n\n").is_ok());
        assert!(event_decoder.feed(b"data: abcdefg\n\n").is_err());
        // Joined data of one event: "ab" + "\n" + "cd" = 5 bytes > 4.
        let mut joined = SseDecoder::new(1024, 4);
        assert!(joined.feed(b"data: ab\ndata: cd\n\n").is_err());
        Ok(())
    }

    #[test]
    fn rejects_oversize_event() {
        let mut decoder = SseDecoder::new(1024, 4);
        assert!(decoder.feed(b"data: too long\n\n").is_err());
    }
}
