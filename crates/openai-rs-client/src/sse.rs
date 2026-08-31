//! Incremental, transport-agnostic decoding for Server-Sent Events (SSE).
//!
//! The decoder accepts arbitrary byte chunks. It deliberately does not depend
//! on `reqwest`: an HTTP transport can feed each response-body chunk to
//! [`SseStreamDecoder::push`] and call [`SseStreamDecoder::finish`] at EOF.
//!
//! Parsing follows the event-stream field rules while placing explicit limits
//! on an incomplete line, assembled event data, and the number of `data`
//! fields. A decoder is fail-stop: after malformed UTF-8 or a limit violation,
//! retained input is released and no later input is accepted.

use std::{mem, str, time::Duration};

use bytes::BytesMut;
use thiserror::Error;

const UTF8_BOM: &[u8; 3] = b"\xef\xbb\xbf";

// Do not duplicate an arbitrarily large caller-owned transport chunk in one
// allocation. Parser-owned memory is bounded by configured limits plus this
// small feed quantum.
const FEED_SLICE_BYTES: usize = 8 * 1024;

/// Default maximum size of one physical SSE line, excluding its terminator.
///
/// This matches [`DEFAULT_MAX_SSE_EVENT_BYTES`] in magnitude. Both official
/// SDKs decode without imposing a line or event size limit, and official
/// payloads such as `response.image_generation_call.partial_image` carry a
/// multi-MiB base64 `data` line in a single physical line, so the DoS boundary
/// is the joined-event limit rather than the line limit.
pub const DEFAULT_MAX_SSE_LINE_BYTES: usize = 32 * 1024 * 1024;

/// Default maximum size of the joined `data` value for one SSE event.
pub const DEFAULT_MAX_SSE_EVENT_BYTES: usize = 32 * 1024 * 1024;

/// Default maximum number of `data` fields in one SSE event.
pub const DEFAULT_MAX_SSE_DATA_LINES: usize = 4096;

/// Resource limits for an incremental SSE decoder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SseLimits {
    max_line_bytes: usize,
    max_event_bytes: usize,
    max_data_lines: usize,
}

impl SseLimits {
    /// Construct validated limits.
    ///
    /// Each limit must be non-zero. `max_event_bytes` limits the final joined
    /// `data` string, including the newline inserted between adjacent `data`
    /// fields. `event`, `id`, comments, and unknown fields are each constrained
    /// by `max_line_bytes` and are never accumulated without replacement.
    pub fn new(
        max_line_bytes: usize,
        max_event_bytes: usize,
        max_data_lines: usize,
    ) -> Result<Self, SseDecodeError> {
        if max_line_bytes == 0 {
            return Err(SseDecodeError::InvalidLimit {
                name: "max_line_bytes",
            });
        }
        if max_event_bytes == 0 {
            return Err(SseDecodeError::InvalidLimit {
                name: "max_event_bytes",
            });
        }
        if max_data_lines == 0 {
            return Err(SseDecodeError::InvalidLimit {
                name: "max_data_lines",
            });
        }

        Ok(Self {
            max_line_bytes,
            max_event_bytes,
            max_data_lines,
        })
    }

    /// Maximum bytes in a physical line, excluding CR/LF terminators.
    pub const fn max_line_bytes(self) -> usize {
        self.max_line_bytes
    }

    /// Maximum bytes in the joined event `data` value.
    pub const fn max_event_bytes(self) -> usize {
        self.max_event_bytes
    }

    /// Maximum number of `data` fields in one event.
    pub const fn max_data_lines(self) -> usize {
        self.max_data_lines
    }
}

impl Default for SseLimits {
    fn default() -> Self {
        Self {
            max_line_bytes: DEFAULT_MAX_SSE_LINE_BYTES,
            max_event_bytes: DEFAULT_MAX_SSE_EVENT_BYTES,
            max_data_lines: DEFAULT_MAX_SSE_DATA_LINES,
        }
    }
}

/// One decoded SSE event.
///
/// `id` is the current last-event ID, so it persists across events until an
/// empty `id:` field resets it. `retry` is present only when this event block
/// contained a valid `retry:` field; the decoder's persistent reconnection
/// delay is available through [`SseDecoder::reconnection_time`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SseFrame {
    /// Value of the final `event:` field in the event block.
    pub event: Option<Box<str>>,
    /// Joined `data:` fields, separated by a single `\n`.
    pub data: Box<str>,
    /// Current last-event ID after processing this event block.
    pub id: Option<Box<str>>,
    /// Reconnection delay supplied in this event block.
    pub retry: Option<Duration>,
}

/// Lifecycle of the low-level parser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SseDecoderState {
    /// More bytes may be supplied.
    Active,
    /// EOF or an explicit local close was processed successfully.
    Finished,
    /// A decoding error occurred and retained input was discarded.
    Failed,
}

/// Lifecycle of an endpoint-aware SSE stream decoder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SseStreamState {
    /// More bytes may be supplied.
    Active,
    /// The configured terminal marker or an allowed EOF was observed.
    Completed,
    /// A configured remote error event was emitted.
    RemoteError,
    /// A decoding error or unexpected EOF occurred.
    Failed,
}

/// Errors raised while decoding an SSE byte stream.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum SseDecodeError {
    /// A decoder limit was configured as zero.
    #[error("SSE limit `{name}` must be non-zero")]
    InvalidLimit {
        /// Name of the invalid limit.
        name: &'static str,
    },

    /// A physical line exceeded the configured limit.
    #[error("SSE line exceeded the {limit}-byte limit")]
    LineTooLarge {
        /// Configured maximum.
        limit: usize,
    },

    /// Joined event data exceeded the configured limit.
    #[error("SSE event data exceeded the {limit}-byte limit")]
    EventTooLarge {
        /// Configured maximum.
        limit: usize,
    },

    /// An event contained too many `data` fields.
    #[error("SSE event exceeded the {limit}-data-line limit")]
    TooManyDataLines {
        /// Configured maximum.
        limit: usize,
    },

    /// A line was not valid UTF-8.
    #[error("SSE line contains invalid UTF-8 at byte {valid_up_to}")]
    InvalidUtf8 {
        /// Number of valid bytes before the invalid sequence.
        valid_up_to: usize,
        /// Length of the invalid sequence, or `None` for an incomplete one.
        error_len: Option<usize>,
    },

    /// The low-level decoder was used after it stopped.
    #[error("SSE decoder is not active (state: {state:?})")]
    DecoderInactive {
        /// State observed by the rejected call.
        state: SseDecoderState,
    },

    /// The endpoint-aware decoder was used after it stopped.
    #[error("SSE stream decoder is not active (state: {state:?})")]
    StreamInactive {
        /// State observed by the rejected call.
        state: SseStreamState,
    },

    /// EOF arrived before an endpoint-required terminal marker.
    #[error("SSE stream ended before {expected}")]
    UnexpectedEof {
        /// Human-readable description of the configured marker.
        expected: Box<str>,
    },
}

/// A bounded incremental SSE parser without endpoint-specific semantics.
#[derive(Debug)]
pub struct SseDecoder {
    limits: SseLimits,
    state: SseDecoderState,
    line_buffer: BytesMut,
    search_from: usize,
    bom_checked: bool,
    event_name: Option<Box<str>>,
    event_data: String,
    data_lines: usize,
    last_event_id: Option<Box<str>>,
    event_retry: Option<Duration>,
    reconnect_retry: Option<Duration>,
}

impl SseDecoder {
    /// Create an active decoder with validated limits.
    pub fn new(limits: SseLimits) -> Self {
        Self {
            limits,
            state: SseDecoderState::Active,
            line_buffer: BytesMut::new(),
            search_from: 0,
            bom_checked: false,
            event_name: None,
            event_data: String::new(),
            data_lines: 0,
            last_event_id: None,
            event_retry: None,
            reconnect_retry: None,
        }
    }

    /// Current parser lifecycle.
    pub const fn state(&self) -> SseDecoderState {
        self.state
    }

    /// Configured parser limits.
    pub const fn limits(&self) -> SseLimits {
        self.limits
    }

    /// Current last-event ID, including an explicitly reset empty value.
    pub fn last_event_id(&self) -> Option<&str> {
        self.last_event_id.as_deref()
    }

    /// Latest valid reconnection delay seen in any `retry:` field.
    pub const fn reconnection_time(&self) -> Option<Duration> {
        self.reconnect_retry
    }

    /// Bytes currently retained for an incomplete line, event, and ID/name.
    ///
    /// This is intended for diagnostics. It excludes allocator spare capacity.
    pub fn buffered_bytes(&self) -> usize {
        self.line_buffer
            .len()
            .saturating_add(self.event_data.len())
            .saturating_add(self.event_name.as_deref().map_or(0, str::len))
            .saturating_add(self.last_event_id.as_deref().map_or(0, str::len))
    }

    /// Feed an arbitrary byte chunk and return every complete event it contains.
    ///
    /// UTF-8 sequences and CRLF terminators may cross calls. An empty chunk is
    /// a no-op. A structural error is fail-stop: the decoder enters
    /// [`SseDecoderState::Failed`] and releases retained input.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseFrame>, SseDecodeError> {
        self.ensure_active()?;

        let result = self.push_inner(chunk);
        if result.is_err() {
            self.fail();
        }
        result
    }

    /// Process EOF and return a final event even when its last block lacks the
    /// customary blank-line delimiter.
    ///
    /// A final unterminated physical line is decoded before the pending event is
    /// flushed. Endpoint-specific policy decides whether EOF itself is success;
    /// see [`SseStreamDecoder::finish`].
    pub fn finish(&mut self) -> Result<Vec<SseFrame>, SseDecodeError> {
        self.ensure_active()?;

        let result = self.finish_inner();
        match result {
            Ok(frames) => {
                self.clear_retained();
                self.state = SseDecoderState::Finished;
                Ok(frames)
            }
            Err(error) => {
                self.fail();
                Err(error)
            }
        }
    }

    /// Discard buffered input and mark this decoder finished.
    ///
    /// This is useful when a higher layer recognizes an endpoint terminal event
    /// before the HTTP body reaches EOF.
    pub fn close(&mut self) {
        if self.state == SseDecoderState::Active {
            self.clear_retained();
            self.state = SseDecoderState::Finished;
        }
    }

    fn ensure_active(&self) -> Result<(), SseDecodeError> {
        if self.state == SseDecoderState::Active {
            Ok(())
        } else {
            Err(SseDecodeError::DecoderInactive { state: self.state })
        }
    }

    fn push_inner(&mut self, chunk: &[u8]) -> Result<Vec<SseFrame>, SseDecodeError> {
        let mut frames = Vec::new();

        for slice in chunk.chunks(FEED_SLICE_BYTES) {
            self.line_buffer.extend_from_slice(slice);
            self.drain_complete_lines(false, &mut frames)?;
            self.check_incomplete_line_limit()?;
        }

        Ok(frames)
    }

    fn finish_inner(&mut self) -> Result<Vec<SseFrame>, SseDecodeError> {
        let mut frames = Vec::new();
        self.drain_complete_lines(true, &mut frames)?;

        if !self.line_buffer.is_empty() {
            let line = self.line_buffer.split();
            self.search_from = 0;
            self.process_line(&line, &mut frames)?;
        }

        if let Some(frame) = self.take_event() {
            frames.push(frame);
        }

        Ok(frames)
    }

    fn drain_complete_lines(
        &mut self,
        eof: bool,
        frames: &mut Vec<SseFrame>,
    ) -> Result<(), SseDecodeError> {
        if !self.ensure_bom_checked(eof) {
            return Ok(());
        }

        while let Some((line_end, terminator_len)) = self.next_line(eof) {
            let next_line = line_end.saturating_add(terminator_len);
            let mut line = self.line_buffer.split_to(next_line);
            line.truncate(line_end);
            self.search_from = 0;
            self.process_line(&line, frames)?;
        }

        Ok(())
    }

    fn ensure_bom_checked(&mut self, eof: bool) -> bool {
        if self.bom_checked {
            return true;
        }

        if !eof
            && self.line_buffer.len() < UTF8_BOM.len()
            && UTF8_BOM.starts_with(&self.line_buffer)
        {
            return false;
        }

        if self.line_buffer.starts_with(UTF8_BOM) {
            let _bom = self.line_buffer.split_to(UTF8_BOM.len());
            self.search_from = self.search_from.saturating_sub(UTF8_BOM.len());
        }
        self.bom_checked = true;
        true
    }

    fn next_line(&mut self, eof: bool) -> Option<(usize, usize)> {
        let mut cursor = self.search_from.min(self.line_buffer.len());

        while cursor < self.line_buffer.len() {
            match self.line_buffer[cursor] {
                b'\n' => return Some((cursor, 1)),
                b'\r' if cursor + 1 < self.line_buffer.len() => {
                    let terminator_len = if self.line_buffer[cursor + 1] == b'\n' {
                        2
                    } else {
                        1
                    };
                    return Some((cursor, terminator_len));
                }
                b'\r' if eof => return Some((cursor, 1)),
                b'\r' => {
                    // Revisit this CR when the next chunk decides lone CR vs
                    // CRLF. No earlier byte needs to be scanned again.
                    self.search_from = cursor;
                    return None;
                }
                _ => cursor += 1,
            }
        }

        self.search_from = self.line_buffer.len();
        None
    }

    fn process_line(
        &mut self,
        raw_line: &[u8],
        frames: &mut Vec<SseFrame>,
    ) -> Result<(), SseDecodeError> {
        if raw_line.len() > self.limits.max_line_bytes {
            return Err(SseDecodeError::LineTooLarge {
                limit: self.limits.max_line_bytes,
            });
        }

        let line = str::from_utf8(raw_line).map_err(|error| SseDecodeError::InvalidUtf8 {
            valid_up_to: error.valid_up_to(),
            error_len: error.error_len(),
        })?;

        if line.is_empty() {
            if let Some(frame) = self.take_event() {
                frames.push(frame);
            }
            return Ok(());
        }

        if line.starts_with(':') {
            return Ok(());
        }

        let (field, mut value) = match line.split_once(':') {
            Some(parts) => parts,
            None => (line, ""),
        };
        if let Some(without_space) = value.strip_prefix(' ') {
            value = without_space;
        }

        match field {
            "event" => {
                self.event_name = if value.is_empty() {
                    None
                } else {
                    Some(value.into())
                };
            }
            "data" => self.push_data(value)?,
            "id" if !value.contains('\0') => self.last_event_id = Some(value.into()),
            "retry" => self.set_retry(value),
            _ => {}
        }

        Ok(())
    }

    fn push_data(&mut self, value: &str) -> Result<(), SseDecodeError> {
        if self.data_lines >= self.limits.max_data_lines {
            return Err(SseDecodeError::TooManyDataLines {
                limit: self.limits.max_data_lines,
            });
        }

        let separator_bytes = usize::from(self.data_lines != 0);
        let next_len = self
            .event_data
            .len()
            .checked_add(separator_bytes)
            .and_then(|len| len.checked_add(value.len()))
            .ok_or(SseDecodeError::EventTooLarge {
                limit: self.limits.max_event_bytes,
            })?;
        if next_len > self.limits.max_event_bytes {
            return Err(SseDecodeError::EventTooLarge {
                limit: self.limits.max_event_bytes,
            });
        }

        if self.data_lines != 0 {
            self.event_data.push('\n');
        }
        self.event_data.push_str(value);
        self.data_lines += 1;
        Ok(())
    }

    fn set_retry(&mut self, value: &str) {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return;
        }
        let Ok(milliseconds) = value.parse::<u64>() else {
            return;
        };
        let delay = Duration::from_millis(milliseconds);
        self.event_retry = Some(delay);
        self.reconnect_retry = Some(delay);
    }

    fn take_event(&mut self) -> Option<SseFrame> {
        if self.data_lines == 0 {
            // A blank line resets per-event fields even when comments/control
            // fields did not create a dispatchable event. ID and the effective
            // reconnection delay intentionally persist.
            self.event_name = None;
            self.event_retry = None;
            return None;
        }

        self.data_lines = 0;
        Some(SseFrame {
            event: self.event_name.take(),
            data: mem::take(&mut self.event_data).into_boxed_str(),
            id: self.last_event_id.clone(),
            retry: self.event_retry.take(),
        })
    }

    fn check_incomplete_line_limit(&self) -> Result<(), SseDecodeError> {
        if !self.bom_checked && UTF8_BOM.starts_with(&self.line_buffer) {
            // Up to two bytes may be the prefix of the stream BOM. They are
            // framing, not line content, and must not consume a tiny custom
            // line limit while the third byte is still in flight.
            return Ok(());
        }

        // A final CR may be a line terminator, so do not count it as content
        // while waiting to see whether the next byte is LF.
        let content_len = if self.line_buffer.last() == Some(&b'\r') {
            self.line_buffer.len().saturating_sub(1)
        } else {
            self.line_buffer.len()
        };
        if content_len > self.limits.max_line_bytes {
            Err(SseDecodeError::LineTooLarge {
                limit: self.limits.max_line_bytes,
            })
        } else {
            Ok(())
        }
    }

    fn clear_retained(&mut self) {
        // Replace, rather than clear, buffers so an attacker-sized allocation
        // is released immediately after completion or failure.
        self.line_buffer = BytesMut::new();
        self.search_from = 0;
        self.event_name = None;
        self.event_data = String::new();
        self.data_lines = 0;
        self.last_event_id = None;
        self.event_retry = None;
        self.reconnect_retry = None;
    }

    fn fail(&mut self) {
        self.clear_retained();
        self.state = SseDecoderState::Failed;
    }
}

impl Default for SseDecoder {
    fn default() -> Self {
        Self::new(SseLimits::default())
    }
}

/// Whether EOF is itself a valid endpoint terminator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SseEofBehavior {
    /// EOF completes the stream even without another marker.
    Complete,
    /// EOF is an error until a configured terminal marker is observed.
    RequireTerminal,
}

/// Endpoint-specific terminal and remote-error rules.
///
/// Terminal SSE event names are returned as [`SseDispatch::Terminal`] so a
/// typed codec can still decode lifecycle events such as
/// `response.completed`. Data sentinels are consumed by this transport layer.
/// Configured remote error events are returned once as
/// [`SseDispatch::RemoteError`] and then close the decoder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SseEndpointPolicy {
    eof: SseEofBehavior,
    terminal_events: Vec<Box<str>>,
    consumed_data_sentinels: Vec<Box<str>>,
    remote_error_events: Vec<Box<str>>,
}

impl SseEndpointPolicy {
    /// Start a custom policy with no event or data matchers.
    pub fn new(eof: SseEofBehavior) -> Self {
        Self {
            eof,
            terminal_events: Vec::new(),
            consumed_data_sentinels: Vec::new(),
            remote_error_events: Vec::new(),
        }
    }

    /// A stream whose clean EOF is its only terminal condition.
    pub fn eof_terminated() -> Self {
        Self::new(SseEofBehavior::Complete)
    }

    /// Legacy Chat/Completions behavior requiring `data: [DONE]`.
    pub fn legacy_done() -> Self {
        Self::new(SseEofBehavior::RequireTerminal)
            .with_consumed_data_sentinel("[DONE]")
            .with_remote_error_event("error")
    }

    /// Responses API lifecycle behavior.
    ///
    /// Stable Responses streams terminate on a lifecycle event. `[DONE]` is
    /// accepted only as a transport-level compatibility sentinel and is never
    /// exposed as a typed Responses event. A standalone `error` event is
    /// surfaced once as a remote error; `response.failed` and
    /// `response.incomplete` remain ordinary typed terminal lifecycle events.
    /// The terminal table matches the 58 pinned `ResponseStreamEvent`
    /// discriminators (no `response.cancelled`, which exists only as a
    /// webhook event) and the typed
    /// [`ResponseStreamEvent::is_terminal`](openai_rs_types::responses::ResponseStreamEvent::is_terminal)
    /// set of completed/failed/incomplete/error.
    pub fn responses() -> Self {
        Self::new(SseEofBehavior::RequireTerminal)
            .with_terminal_event("response.completed")
            .with_terminal_event("response.failed")
            .with_terminal_event("response.incomplete")
            .with_consumed_data_sentinel("[DONE]")
            .with_remote_error_event("error")
    }

    /// Add an SSE event name that completes the stream after being emitted.
    pub fn with_terminal_event(mut self, event: impl Into<Box<str>>) -> Self {
        self.terminal_events.push(event.into());
        self
    }

    /// Add an exact `data` sentinel consumed by the transport layer.
    pub fn with_consumed_data_sentinel(mut self, data: impl Into<Box<str>>) -> Self {
        self.consumed_data_sentinels.push(data.into());
        self
    }

    /// Add an SSE event name that is emitted once as a remote stream error.
    pub fn with_remote_error_event(mut self, event: impl Into<Box<str>>) -> Self {
        self.remote_error_events.push(event.into());
        self
    }

    /// Configured EOF behavior.
    pub const fn eof_behavior(&self) -> SseEofBehavior {
        self.eof
    }

    fn classify(&self, frame: &SseFrame) -> FrameClassification {
        let event = frame.event.as_deref();
        if event.is_some_and(|name| contains(&self.remote_error_events, name)) {
            FrameClassification::RemoteError
        } else if contains(&self.consumed_data_sentinels, &frame.data) {
            FrameClassification::ConsumedTerminal
        } else if event.is_some_and(|name| contains(&self.terminal_events, name)) {
            FrameClassification::EmittedTerminal
        } else {
            FrameClassification::Event
        }
    }

    fn expected_terminal(&self) -> Box<str> {
        let mut expected = Vec::new();
        for event in &self.terminal_events {
            expected.push(format!("event `{event}`"));
        }
        for data in &self.consumed_data_sentinels {
            expected.push(format!("data sentinel `{data}`"));
        }

        if expected.is_empty() {
            "the endpoint's required terminal marker".into()
        } else {
            expected.join(" or ").into_boxed_str()
        }
    }
}

fn contains(values: &[Box<str>], candidate: &str) -> bool {
    values.iter().any(|value| value.as_ref() == candidate)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameClassification {
    Event,
    EmittedTerminal,
    ConsumedTerminal,
    RemoteError,
}

/// An endpoint-aware decoder output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SseDispatch {
    /// A non-terminal event.
    Event(SseFrame),
    /// A typed terminal lifecycle event; emit it before ending the stream.
    Terminal(SseFrame),
    /// An in-band remote error event; convert it to the endpoint error type and
    /// then end the stream.
    RemoteError(SseFrame),
}

impl SseDispatch {
    /// Borrow the underlying frame regardless of classification.
    pub const fn frame(&self) -> &SseFrame {
        match self {
            Self::Event(frame) | Self::Terminal(frame) | Self::RemoteError(frame) => frame,
        }
    }

    /// Consume the dispatch and return its frame.
    pub fn into_frame(self) -> SseFrame {
        match self {
            Self::Event(frame) | Self::Terminal(frame) | Self::RemoteError(frame) => frame,
        }
    }

    /// Whether this dispatch closes the endpoint stream.
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_) | Self::RemoteError(_))
    }
}

/// A bounded SSE decoder with endpoint-specific completion semantics.
#[derive(Debug)]
pub struct SseStreamDecoder {
    decoder: SseDecoder,
    policy: SseEndpointPolicy,
    state: SseStreamState,
}

impl SseStreamDecoder {
    /// Create a decoder from explicit limits and endpoint policy.
    pub fn new(limits: SseLimits, policy: SseEndpointPolicy) -> Self {
        Self {
            decoder: SseDecoder::new(limits),
            policy,
            state: SseStreamState::Active,
        }
    }

    /// Create a decoder using [`SseLimits::default`].
    pub fn with_default_limits(policy: SseEndpointPolicy) -> Self {
        Self::new(SseLimits::default(), policy)
    }

    /// Current endpoint stream lifecycle.
    pub const fn state(&self) -> SseStreamState {
        self.state
    }

    /// Endpoint policy in force for this decoder.
    pub const fn policy(&self) -> &SseEndpointPolicy {
        &self.policy
    }

    /// Underlying parser, for reconnection metadata and diagnostics.
    pub const fn decoder(&self) -> &SseDecoder {
        &self.decoder
    }

    /// Feed one transport chunk.
    ///
    /// Once a terminal or remote-error dispatch is returned, later frames from
    /// the same chunk are ignored and parser-owned input is released. The HTTP
    /// layer should immediately drop/close the authenticated response body.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseDispatch>, SseDecodeError> {
        self.ensure_active()?;
        let mut dispatches = Vec::new();
        let mut offset = 0;

        // Feed no farther than the next physical line boundary. In particular,
        // do not parse attacker-controlled bytes that follow a terminal marker
        // in the same HTTP chunk. A trailing CR needs one byte of look-ahead;
        // supply only that byte first so a lone-CR blank line can terminate the
        // endpoint before any later line is decoded.
        while offset < chunk.len() {
            let end = if self.decoder.line_buffer.last() == Some(&b'\r') {
                offset.saturating_add(1)
            } else {
                chunk[offset..]
                    .iter()
                    .position(|byte| matches!(byte, b'\r' | b'\n'))
                    .map_or(chunk.len(), |relative| offset + relative + 1)
            };

            let frames = match self.decoder.push(&chunk[offset..end]) {
                Ok(frames) => frames,
                Err(error) => {
                    self.state = SseStreamState::Failed;
                    return Err(error);
                }
            };
            dispatches.extend(self.classify_frames(frames));
            if self.state != SseStreamState::Active {
                break;
            }
            offset = end;
        }

        Ok(dispatches)
    }

    /// Process EOF according to the endpoint policy.
    ///
    /// A required terminal marker missing at EOF is a fail-stop
    /// [`SseDecodeError::UnexpectedEof`].
    pub fn finish(&mut self) -> Result<Vec<SseDispatch>, SseDecodeError> {
        self.ensure_active()?;
        let frames = match self.decoder.finish() {
            Ok(frames) => frames,
            Err(error) => {
                self.state = SseStreamState::Failed;
                return Err(error);
            }
        };
        let dispatches = self.classify_frames(frames);

        if self.state != SseStreamState::Active {
            return Ok(dispatches);
        }

        match self.policy.eof {
            SseEofBehavior::Complete => {
                self.state = SseStreamState::Completed;
                Ok(dispatches)
            }
            SseEofBehavior::RequireTerminal => {
                self.state = SseStreamState::Failed;
                Err(SseDecodeError::UnexpectedEof {
                    expected: self.policy.expected_terminal(),
                })
            }
        }
    }

    /// Discard buffered input because the caller stopped early.
    pub fn close(&mut self) {
        if self.state == SseStreamState::Active {
            self.decoder.close();
            self.state = SseStreamState::Completed;
        }
    }

    fn ensure_active(&self) -> Result<(), SseDecodeError> {
        if self.state == SseStreamState::Active {
            Ok(())
        } else {
            Err(SseDecodeError::StreamInactive { state: self.state })
        }
    }

    fn classify_frames(&mut self, frames: Vec<SseFrame>) -> Vec<SseDispatch> {
        let mut dispatches = Vec::with_capacity(frames.len());

        for frame in frames {
            match self.policy.classify(&frame) {
                FrameClassification::Event => dispatches.push(SseDispatch::Event(frame)),
                FrameClassification::EmittedTerminal => {
                    dispatches.push(SseDispatch::Terminal(frame));
                    self.decoder.close();
                    self.state = SseStreamState::Completed;
                    break;
                }
                FrameClassification::ConsumedTerminal => {
                    self.decoder.close();
                    self.state = SseStreamState::Completed;
                    break;
                }
                FrameClassification::RemoteError => {
                    dispatches.push(SseDispatch::RemoteError(frame));
                    self.decoder.close();
                    self.state = SseStreamState::RemoteError;
                    break;
                }
            }
        }

        dispatches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T>(result: Result<T, SseDecodeError>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("unexpected SSE error: {error}"),
        }
    }

    fn one(mut frames: Vec<SseFrame>) -> SseFrame {
        assert_eq!(frames.len(), 1);
        match frames.pop() {
            Some(frame) => frame,
            None => panic!("expected one SSE frame"),
        }
    }

    fn decode_chunks(input: &[u8], split_at: usize) -> Vec<SseFrame> {
        let mut decoder = SseDecoder::default();
        let mut frames = ok(decoder.push(&input[..split_at]));
        frames.extend(ok(decoder.push(&input[split_at..])));
        frames.extend(ok(decoder.finish()));
        frames
    }

    #[test]
    fn decodes_every_two_chunk_split_including_utf8_and_crlf() {
        let input = "\u{feff}: keepalive\r\nevent: 文本\r\nid: 标识\r\nretry: 1250\r\ndata: 你\r\ndata: 好\r\n\r\n"
            .as_bytes();
        let expected = SseFrame {
            event: Some("文本".into()),
            data: "你\n好".into(),
            id: Some("标识".into()),
            retry: Some(Duration::from_millis(1250)),
        };

        for split_at in 0..=input.len() {
            assert_eq!(decode_chunks(input, split_at), vec![expected.clone()]);
        }
    }

    #[test]
    fn decodes_one_byte_at_a_time() {
        let input = b"data: first\r\rdata: second\r\n\r\ndata: third\n\n";
        let mut decoder = SseDecoder::default();
        let mut frames = Vec::new();
        for byte in input {
            frames.extend(ok(decoder.push(&[*byte])));
        }
        frames.extend(ok(decoder.finish()));

        assert_eq!(
            frames
                .iter()
                .map(|frame| frame.data.as_ref())
                .collect::<Vec<_>>(),
            vec!["first", "second", "third"]
        );
    }

    #[test]
    fn supports_event_id_retry_and_multiline_data() {
        let mut decoder = SseDecoder::default();
        let frame = one(ok(
            decoder.push(b"event: update\nid: 42\nretry: 3000\ndata: one\ndata:two\n\n")
        ));

        assert_eq!(frame.event.as_deref(), Some("update"));
        assert_eq!(&*frame.data, "one\ntwo");
        assert_eq!(frame.id.as_deref(), Some("42"));
        assert_eq!(frame.retry, Some(Duration::from_secs(3)));
        assert_eq!(decoder.last_event_id(), Some("42"));
        assert_eq!(decoder.reconnection_time(), Some(Duration::from_secs(3)));
    }

    #[test]
    fn ignores_comments_unknown_fields_and_control_only_blocks() {
        let mut decoder = SseDecoder::default();
        let frames =
            ok(decoder.push(b": ping\nunknown: value\nevent: ignored-without-data\n\n: pong\n\n"));
        assert!(frames.is_empty());

        let frame = one(ok(decoder.push(b"data: visible\n\n")));
        assert_eq!(frame.event, None);
        assert_eq!(&*frame.data, "visible");
    }

    #[test]
    fn dispatches_an_explicit_empty_data_field() {
        let mut decoder = SseDecoder::default();
        let frame = one(ok(decoder.push(b"data:\n\n")));
        assert_eq!(&*frame.data, "");
    }

    #[test]
    fn an_empty_event_field_restores_the_default_event_type() {
        let mut decoder = SseDecoder::default();
        let frame = one(ok(decoder.push(b"event: custom\nevent:\ndata: payload\n\n")));
        assert_eq!(frame.event, None);
    }

    #[test]
    fn id_persists_can_be_reset_and_rejects_null() {
        let mut decoder = SseDecoder::default();
        assert!(ok(decoder.push(b"id: good\n\n")).is_empty());

        let first = one(ok(decoder.push(b"id: bad\0value\ndata: first\n\n")));
        assert_eq!(first.id.as_deref(), Some("good"));

        let second = one(ok(decoder.push(b"id:\ndata: second\n\n")));
        assert_eq!(second.id.as_deref(), Some(""));

        let third = one(ok(decoder.push(b"data: third\n\n")));
        assert_eq!(third.id.as_deref(), Some(""));
    }

    #[test]
    fn retry_requires_ascii_digits_and_persists_for_reconnection() {
        let mut decoder = SseDecoder::default();
        assert!(ok(decoder.push(b"retry: 25\n\n")).is_empty());
        assert_eq!(decoder.reconnection_time(), Some(Duration::from_millis(25)));

        let first = one(ok(decoder.push(b"retry: +30\ndata: first\n\n")));
        assert_eq!(first.retry, None);
        assert_eq!(decoder.reconnection_time(), Some(Duration::from_millis(25)));

        let second = one(ok(decoder.push(b"retry: 40\ndata: second\n\n")));
        assert_eq!(second.retry, Some(Duration::from_millis(40)));
        assert_eq!(decoder.reconnection_time(), Some(Duration::from_millis(40)));
    }

    #[test]
    fn finish_flushes_an_unterminated_final_event() {
        let mut decoder = SseDecoder::default();
        assert!(ok(decoder.push(b"event: final\ndata: payload")).is_empty());
        let frame = one(ok(decoder.finish()));
        assert_eq!(frame.event.as_deref(), Some("final"));
        assert_eq!(&*frame.data, "payload");
        assert_eq!(decoder.state(), SseDecoderState::Finished);
    }

    #[test]
    fn a_split_bom_is_removed_only_at_stream_start() {
        let mut decoder = SseDecoder::default();
        assert!(ok(decoder.push(&UTF8_BOM[..1])).is_empty());
        assert!(ok(decoder.push(&UTF8_BOM[1..])).is_empty());
        let first = one(ok(decoder.push(b"data: first\n\n")));
        assert_eq!(&*first.data, "first");

        let mut later = b"data: ".to_vec();
        later.extend_from_slice(UTF8_BOM);
        later.extend_from_slice(b"second\n\n");
        let second = one(ok(decoder.push(&later)));
        assert_eq!(&*second.data, "\u{feff}second");
    }

    #[test]
    fn malformed_utf8_is_fail_stop_and_releases_buffers() {
        let mut decoder = SseDecoder::default();
        let error = decoder.push(b"data: \xff\n\n");
        assert!(matches!(error, Err(SseDecodeError::InvalidUtf8 { .. })));
        assert_eq!(decoder.state(), SseDecoderState::Failed);
        assert_eq!(decoder.buffered_bytes(), 0);
        assert_eq!(
            decoder.push(b"data: later\n\n"),
            Err(SseDecodeError::DecoderInactive {
                state: SseDecoderState::Failed,
            })
        );
    }

    #[test]
    fn enforces_line_event_and_data_line_limits() {
        let line_limits = ok(SseLimits::new(4, 100, 10));
        let mut line_decoder = SseDecoder::new(line_limits);
        assert_eq!(
            line_decoder.push(b"abcde"),
            Err(SseDecodeError::LineTooLarge { limit: 4 })
        );

        let event_limits = ok(SseLimits::new(32, 3, 10));
        let mut event_decoder = SseDecoder::new(event_limits);
        assert_eq!(
            event_decoder.push(b"data: ab\ndata: cd\n\n"),
            Err(SseDecodeError::EventTooLarge { limit: 3 })
        );

        let data_line_limits = ok(SseLimits::new(32, 100, 1));
        let mut data_line_decoder = SseDecoder::new(data_line_limits);
        assert_eq!(
            data_line_decoder.push(b"data: a\ndata: b\n\n"),
            Err(SseDecodeError::TooManyDataLines { limit: 1 })
        );
    }

    #[test]
    fn invalid_limits_are_rejected() {
        assert_eq!(
            SseLimits::new(0, 1, 1),
            Err(SseDecodeError::InvalidLimit {
                name: "max_line_bytes",
            })
        );
        assert_eq!(
            SseLimits::new(1, 0, 1),
            Err(SseDecodeError::InvalidLimit {
                name: "max_event_bytes",
            })
        );
        assert_eq!(
            SseLimits::new(1, 1, 0),
            Err(SseDecodeError::InvalidLimit {
                name: "max_data_lines",
            })
        );
    }

    #[test]
    fn default_line_limit_matches_the_event_limit() {
        // Both official SDKs impose no line or event size limit, so the line
        // limit must stay in the same magnitude class as the event limit that
        // actually bounds memory; a 1 MiB line limit rejected official
        // single-line `partial_image` base64 payloads.
        assert_eq!(DEFAULT_MAX_SSE_LINE_BYTES, 32 * 1024 * 1024);
        assert_eq!(DEFAULT_MAX_SSE_LINE_BYTES, DEFAULT_MAX_SSE_EVENT_BYTES);
        assert_eq!(
            SseLimits::default().max_line_bytes(),
            DEFAULT_MAX_SSE_LINE_BYTES
        );
    }

    #[test]
    fn decodes_a_single_data_line_above_one_mebibyte() {
        // `response.image_generation_call.partial_image` delivers a multi-MiB
        // base64 payload as one physical `data:` line, which the previous 1 MiB
        // default line limit rejected before the event limit was ever reached.
        const PAYLOAD_BYTES: usize = 1024 * 1024 + 4096;

        let mut input = Vec::with_capacity(PAYLOAD_BYTES + "data: \n\n".len());
        input.extend_from_slice(b"data: ");
        input.extend(std::iter::repeat_n(b'A', PAYLOAD_BYTES));
        input.extend_from_slice(b"\n\n");

        let mut decoder = SseDecoder::default();
        // Cross the internal 8 KiB feed quantum and an arbitrary chunk boundary
        // so the incomplete-line limit check also sees the oversized line.
        let mut frames = ok(decoder.push(&input[..64 * 1024]));
        frames.extend(ok(decoder.push(&input[64 * 1024..])));
        frames.extend(ok(decoder.finish()));

        let frame = one(frames);
        assert_eq!(frame.data.len(), PAYLOAD_BYTES);
        assert!(frame.data.bytes().all(|byte| byte == b'A'));
    }

    #[test]
    fn legacy_done_is_consumed_and_closes_the_decoder() {
        let mut decoder = SseStreamDecoder::with_default_limits(SseEndpointPolicy::legacy_done());
        let dispatches =
            ok(decoder.push(b"data: {\"value\":1}\n\ndata: [DONE]\n\ndata: ignored\n\n"));
        assert_eq!(dispatches.len(), 1);
        assert!(matches!(dispatches[0], SseDispatch::Event(_)));
        assert_eq!(decoder.state(), SseStreamState::Completed);
        assert_eq!(
            decoder.push(b"data: later\n\n"),
            Err(SseDecodeError::StreamInactive {
                state: SseStreamState::Completed,
            })
        );
    }

    #[test]
    fn bytes_after_a_terminal_in_the_same_chunk_are_not_decoded() {
        let mut decoder = SseStreamDecoder::with_default_limits(SseEndpointPolicy::legacy_done());
        let dispatches = ok(decoder.push(b"data: first\n\ndata: [DONE]\n\ndata: \xff\n\n"));
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].frame().data.as_ref(), "first");
        assert_eq!(decoder.state(), SseStreamState::Completed);
    }

    #[test]
    fn a_lone_cr_terminal_stops_before_the_following_line() {
        let mut decoder = SseStreamDecoder::with_default_limits(SseEndpointPolicy::legacy_done());
        let dispatches = ok(decoder.push(b"data: [DONE]\r\rdata: \xff\n"));
        assert!(dispatches.is_empty());
        assert_eq!(decoder.state(), SseStreamState::Completed);
    }

    #[test]
    fn responses_terminal_event_is_emitted_before_completion() {
        for terminal in [
            "response.completed",
            "response.failed",
            "response.incomplete",
        ] {
            let mut decoder = SseStreamDecoder::with_default_limits(SseEndpointPolicy::responses());
            let input = format!("event: {terminal}\ndata: {{\"type\":\"{terminal}\"}}\n\n");
            let dispatches = ok(decoder.push(input.as_bytes()));
            assert_eq!(dispatches.len(), 1);
            assert!(matches!(dispatches[0], SseDispatch::Terminal(_)));
            assert_eq!(dispatches[0].frame().event.as_deref(), Some(terminal));
            assert_eq!(decoder.state(), SseStreamState::Completed);
        }
    }

    #[test]
    fn responses_terminal_table_matches_pinned_stream_events() {
        // The pinned ResponseStreamEvent union has 58 discriminators and no
        // `response.cancelled` (that tag exists only as a webhook event), so
        // the SSE policy must treat it as an ordinary event rather than a
        // terminator, mirroring types-side `is_terminal()`.
        let mut decoder = SseStreamDecoder::with_default_limits(SseEndpointPolicy::responses());
        let dispatches = ok(decoder.push(
            concat!(
                "event: response.cancelled\n",
                "data: {\"type\":\"response.cancelled\"}\n\n",
                "event: response.completed\n",
                "data: {\"type\":\"response.completed\"}\n\n",
            )
            .as_bytes(),
        ));
        assert_eq!(dispatches.len(), 2);
        assert!(matches!(dispatches[0], SseDispatch::Event(_)));
        assert!(matches!(dispatches[1], SseDispatch::Terminal(_)));
        assert_eq!(decoder.state(), SseStreamState::Completed);
    }

    #[test]
    fn remote_error_is_emitted_once_and_then_stops() {
        let mut decoder = SseStreamDecoder::with_default_limits(SseEndpointPolicy::responses());
        let dispatches = ok(decoder.push(
            b"event: response.created\ndata: {}\n\nevent: error\ndata: {\"message\":\"bad\"}\n\nevent: response.completed\ndata: {}\n\n",
        ));
        assert_eq!(dispatches.len(), 2);
        assert!(matches!(dispatches[0], SseDispatch::Event(_)));
        assert!(matches!(dispatches[1], SseDispatch::RemoteError(_)));
        assert_eq!(decoder.state(), SseStreamState::RemoteError);
        assert_eq!(
            decoder.finish(),
            Err(SseDecodeError::StreamInactive {
                state: SseStreamState::RemoteError,
            })
        );
    }

    #[test]
    fn required_terminal_reports_unexpected_eof() {
        let mut decoder = SseStreamDecoder::with_default_limits(SseEndpointPolicy::responses());
        let dispatches = ok(decoder.push(b"event: response.created\ndata: {}\n\n"));
        assert_eq!(dispatches.len(), 1);

        let error = decoder.finish();
        assert!(matches!(error, Err(SseDecodeError::UnexpectedEof { .. })));
        assert_eq!(decoder.state(), SseStreamState::Failed);
    }

    #[test]
    fn eof_policy_completes_without_a_marker() {
        let mut decoder =
            SseStreamDecoder::with_default_limits(SseEndpointPolicy::eof_terminated());
        let dispatches = ok(decoder.push(b"data: final"));
        assert!(dispatches.is_empty());
        let dispatches = ok(decoder.finish());
        assert_eq!(dispatches.len(), 1);
        assert_eq!(decoder.state(), SseStreamState::Completed);
    }

    #[test]
    fn terminal_event_without_trailing_blank_line_is_recognized_at_eof() {
        let mut decoder = SseStreamDecoder::with_default_limits(SseEndpointPolicy::responses());
        assert!(
            ok(decoder
                .push(b"event: response.completed\ndata: {\"type\":\"response.completed\"}",))
            .is_empty()
        );
        let dispatches = ok(decoder.finish());
        assert_eq!(dispatches.len(), 1);
        assert!(matches!(dispatches[0], SseDispatch::Terminal(_)));
        assert_eq!(decoder.state(), SseStreamState::Completed);
    }
}
