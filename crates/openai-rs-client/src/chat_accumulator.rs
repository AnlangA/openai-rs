//! Fold engine reducing a Chat Completions chunk stream into one completion.
//!
//! Both official SDKs accumulate `stream: true` chunks into a `ChatCompletion`
//! snapshot alongside the raw stream. openai-python exposes
//! `ChatCompletionStreamState` (`lib/streaming/chat/_completions.py:292`) with
//! `handle_chunk` / `current_completion_snapshot` / `get_final_completion` and
//! folds with the generic `accumulate_delta` engine
//! (`lib/streaming/_deltas.py:6`), which concatenates string deltas and merges
//! `tool_calls` entries by their chunk `index`. openai-node keeps
//! `#currentChatCompletionSnapshot` and `#accumulateChatCompletion` on
//! `ChatCompletionStream` (`src/lib/ChatCompletionStream.ts:1817`).
//! [`ChatCompletionAccumulator`] is the Rust mirror over typed chunks, and
//! [`ChatCompletionEventStream::collect_with`](crate::ChatCompletionEventStream::collect_with)
//! drives it to a terminal [`ChatCompletion`](openai_rs_types::chat::ChatCompletion).

use std::collections::BTreeMap;

use openai_rs_types::chat::{
    ChatCompletion, ChatCompletionChunk, ChatCompletionChunkChoice, ChatCompletionObject,
    ChatFinishReason, ChatRole, ChatToolCallChunk, ChatToolKind,
};
use openai_rs_types::{ModelId, Nullable, Omittable};
use serde::Serialize;
use serde_json::{Map, Number, Value};

use crate::{BodyPreview, Error};

/// Stateful reducer folding Chat Completions chunks into a completion.
///
/// Fold rules mirror the official SDKs:
///
/// - `content` / `refusal` deltas concatenate (openai-python
///   `lib/streaming/_deltas.py:27`, openai-node `ChatCompletionStream.ts:1933`);
///   an explicit `null` marks the field seen without contributing text.
/// - `tool_calls` entries are keyed by the required chunk `index`
///   (`_deltas.py:44-60`); `id`, `type` and `function.name` are announced once,
///   so the first observed value wins (openai-node overwrites on a truthy
///   repeat and openai-python would concatenate — neither is observable on a
///   conforming stream), while `function.arguments` accumulates. Custom-tool
///   `custom` payloads are not typed on the chunk model and ride its extra
///   fields, where the `_deltas.py` object merge concatenates `input` the same
///   way.
/// - The deprecated `function_call` delta folds `arguments` the same way
///   (`ChatCompletionStream.ts:1955-1967`).
/// - `role` and `finish_reason` arrive on whichever chunk carries them and the
///   last non-empty value wins (`ChatCompletionStream.ts:1888` and `:1937`;
///   openai-python assigns per chunk in `_completions.py:424`).
/// - The final usage-only chunk (empty `choices` plus populated `usage`)
///   replaces `usage`; `model` / `id` / `created` / `service_tier` /
///   `moderation` take their first observed value (openai-python seeds the
///   snapshot from the first chunk, `_completions.py:740`; openai-node keeps
///   re-assigning identical values, `ChatCompletionStream.ts:1830`).
/// - `system_fingerprint` is re-read from every chunk that carries it
///   (`_completions.py:489`); `obfuscation` is dropped like both SDKs.
/// - `logprobs.content` / `logprobs.refusal` token lists extend
///   (`ChatCompletionStream.ts:1860-1885`).
///
/// Chunks whose `object` is not `chat.completion.chunk` are ignored, matching
/// openai-python's Azure asynchronous-filter workaround
/// (`_completions.py:764-770`).
///
/// ```
/// use openai_rs_client::ChatCompletionAccumulator;
/// use openai_rs_types::chat::ChatCompletionChunk;
///
/// let chunk: ChatCompletionChunk = serde_json::from_str(
///     r#"{"id":"chatcmpl_1","choices":[{"delta":{"role":"assistant","content":null},
///        "finish_reason":null,"index":0}],"created":1,"model":"gpt-4o",
///        "object":"chat.completion.chunk"}"#,
/// )
/// .expect("decode chunk");
///
/// let mut accumulator = ChatCompletionAccumulator::new();
/// accumulator.push(&chunk);
/// let snapshot = accumulator.snapshot().expect("snapshot after first chunk");
/// assert_eq!(snapshot.choices[0].message.role.as_str(), "assistant");
/// ```
#[derive(Debug, Clone, Default)]
pub struct ChatCompletionAccumulator {
    top: TopLevelState,
    choices: BTreeMap<u32, ChoiceState>,
    /// Set once any chunk has been observed.
    started: bool,
    /// Set when the transport observed the terminal `[DONE]` sentinel.
    done: bool,
}

impl ChatCompletionAccumulator {
    /// Creates an empty accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Folds one decoded chunk into the accumulated state.
    ///
    /// Chunks whose `object` discriminator is not `chat.completion.chunk` are
    /// skipped, mirroring openai-python's `_is_valid_chat_completion_chunk_weak`
    /// filter for Azure's asynchronous-filter events
    /// (`lib/streaming/chat/_completions.py:764`).
    pub fn push(&mut self, chunk: &ChatCompletionChunk) {
        if chunk.object != ChatCompletionObject::Chunk {
            return;
        }
        self.started = true;
        self.top.observe(chunk);
        for choice in &chunk.choices {
            self.choices
                .entry(choice.index)
                .or_default()
                .observe(choice);
        }
    }

    /// Builds the accumulated completion from the state observed so far.
    ///
    /// Returns `None` before the first chunk. This is the Rust form of
    /// openai-python's `current_completion_snapshot` and openai-node's
    /// `currentChatCompletionSnapshot`: it is valid to call mid-stream.
    ///
    /// The pinned non-streaming model has no null state for two fields the
    /// service only fills in later, so an in-progress snapshot reports typed
    /// stand-ins there: a choice that has not announced `finish_reason` reads
    /// as [`ChatFinishReason::Stop`] (both SDKs keep their loose `null` until
    /// the terminal chunk; use [`is_done`](Self::is_done) or
    /// [`finish_reason`](Self::finish_reason) for the precise signal) and a
    /// message that has not announced `role` reads as
    /// [`ChatRole::Assistant`], the only role a generated choice can carry.
    #[must_use]
    pub fn snapshot(&self) -> Option<ChatCompletion> {
        if !self.started {
            return None;
        }
        // The snapshot decodes from folded JSON, mirroring openai-python's
        // `construct_type` over the accumulated dict (`_completions.py:371`).
        // Unknown passthrough fields can in principle take shapes the typed
        // model rejects (a partial `audio` object, say), so a failing decode
        // is retried without any passthrough fields: the typed fold always
        // yields a snapshot.
        serde_json::from_value(Value::Object(self.snapshot_object(true)))
            .ok()
            .or_else(|| serde_json::from_value(Value::Object(self.snapshot_object(false))).ok())
    }

    /// Alias of [`snapshot`](Self::snapshot), named after openai-python's
    /// `current_completion_snapshot`.
    #[must_use]
    pub fn current(&self) -> Option<ChatCompletion> {
        self.snapshot()
    }

    /// Returns the stop reason observed for one choice, if any.
    #[must_use]
    pub fn finish_reason(&self, choice_index: u32) -> Option<ChatFinishReason> {
        self.choices
            .get(&choice_index)
            .and_then(|choice| choice.finish_reason.clone())
    }

    /// Returns whether the fold has observed a terminal condition: any
    /// choice's `finish_reason`, or the `[DONE]` sentinel.
    ///
    /// The SSE transport consumes `[DONE]` itself
    /// (`SseEndpointPolicy::legacy_done`), so it never arrives as a chunk;
    /// [`mark_done`](Self::mark_done) records it for manually driven folds and
    /// [`ChatCompletionEventStream::collect_with`](crate::ChatCompletionEventStream::collect_with)
    /// calls it automatically once the stream ends without an error.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.done || self.choices.values().any(|c| c.finish_reason.is_some())
    }

    /// Records that the transport observed the stream's terminal `[DONE]`
    /// sentinel (or an equivalent clean end of stream).
    pub fn mark_done(&mut self) {
        self.done = true;
    }

    /// Returns the accumulated completion, requiring a terminal condition.
    ///
    /// Mirrors openai-python's `get_final_completion`
    /// (`lib/streaming/chat/_completions.py:93`) with the strictness of the
    /// Responses channel's `ResponseAccumulator::finish`: folding a stream
    /// that ended without any `finish_reason` and without `[DONE]` is an
    /// error rather than a silently partial completion.
    pub fn finish(self) -> Result<ChatCompletion, Error> {
        if !self.is_done() {
            return Err(Error::StreamProtocol {
                message: "the chat completion stream ended before any finish reason or [DONE] \
                          sentinel was observed",
                request_id: None,
                body: BodyPreview::from_bytes(&[], false),
            });
        }
        self.snapshot().ok_or(Error::StreamProtocol {
            message: "no chat completion chunk was observed before the stream ended",
            request_id: None,
            body: BodyPreview::from_bytes(&[], false),
        })
    }

    fn snapshot_object(&self, passthrough: bool) -> Map<String, Value> {
        let top = &self.top;
        let mut object = Map::new();
        object.insert(
            String::from("id"),
            Value::String(top.id.clone().unwrap_or_default()),
        );
        object.insert(
            String::from("created"),
            Value::Number(Number::from(top.created.unwrap_or(0))),
        );
        object.insert(
            String::from("model"),
            Value::String(
                top.model
                    .as_ref()
                    .map(|model| model.as_str().to_owned())
                    .unwrap_or_default(),
            ),
        );
        // The snapshot carries the non-streaming discriminator, as
        // openai-python rewrites it (`_completions.py:758`).
        object.insert(
            String::from("object"),
            Value::String(String::from("chat.completion")),
        );
        let choices = self
            .choices
            .iter()
            .map(|(index, choice)| choice.snapshot(*index, passthrough))
            .collect::<Vec<_>>();
        object.insert(String::from("choices"), Value::Array(choices));
        if passthrough {
            insert_first_seen(&mut object, &top.extra);
        }
        if let Some(service_tier) = &top.service_tier {
            object.insert(String::from("service_tier"), service_tier.clone());
        }
        if let Some(system_fingerprint) = &top.system_fingerprint {
            object.insert(
                String::from("system_fingerprint"),
                Value::String(system_fingerprint.clone()),
            );
        }
        if let Some(usage) = &top.usage {
            object.insert(String::from("usage"), usage.clone());
        }
        if let Some(moderation) = &top.moderation {
            object.insert(String::from("moderation"), moderation.clone());
        }
        object
    }
}

/// Completion-level state shared by every chunk.
#[derive(Debug, Clone, Default)]
struct TopLevelState {
    id: Option<String>,
    created: Option<u64>,
    model: Option<ModelId>,
    service_tier: Option<Value>,
    system_fingerprint: Option<String>,
    usage: Option<Value>,
    moderation: Option<Value>,
    /// Unknown top-level chunk fields; first observed value wins.
    extra: Map<String, Value>,
}

impl TopLevelState {
    fn observe(&mut self, chunk: &ChatCompletionChunk) {
        // First observed value wins, as openai-python seeds these from the
        // first chunk (`lib/streaming/chat/_completions.py:740`) and the
        // service repeats them verbatim afterwards.
        if self.id.is_none() {
            self.id = Some(chunk.id.clone());
        }
        if self.created.is_none() {
            self.created = Some(chunk.created);
        }
        if self.model.is_none() {
            self.model = Some(chunk.model.clone());
        }
        if self.service_tier.is_none() {
            if let Omittable::Value(tier) = &chunk.service_tier {
                self.service_tier = Some(to_value_lossy(tier));
            }
        }
        if self.moderation.is_none() {
            if let Omittable::Value(moderation) = &chunk.moderation {
                self.moderation = Some(to_value_lossy(moderation));
            }
        }
        // openai-python re-assigns the fingerprint on every chunk
        // (`_completions.py:489`); absent fields leave the fold untouched.
        if let Omittable::Value(fingerprint) = &chunk.system_fingerprint {
            self.system_fingerprint = Some(fingerprint.clone());
        }
        // Usage is null on ordinary chunks and populated on the final
        // usage-only chunk; the last populated value wins.
        if let Omittable::Value(Nullable::Value(usage)) = &chunk.usage {
            self.usage = Some(to_value_lossy(usage));
        }
        for (key, value) in chunk.extra().iter() {
            self.extra
                .entry(key.to_owned())
                .or_insert_with(|| value.clone());
        }
    }
}

/// Per-choice fold state, addressed by the chunk `index`.
#[derive(Debug, Clone, Default)]
struct ChoiceState {
    role: Option<ChatRole>,
    content: Option<String>,
    refusal: Option<Nullable<String>>,
    finish_reason: Option<ChatFinishReason>,
    tool_calls_seen: bool,
    tool_calls: BTreeMap<u32, ToolCallState>,
    legacy_function_call: Option<LegacyFunctionCallState>,
    logprobs: Option<LogprobsState>,
    /// Unknown choice-level fields; first observed value wins.
    extra: Map<String, Value>,
    /// Unknown delta fields, deep-merged per `_deltas.py`.
    message_extra: Map<String, Value>,
}

impl ChoiceState {
    fn observe(&mut self, choice: &ChatCompletionChunkChoice) {
        // Last non-empty stop reason wins (openai-node
        // `ChatCompletionStream.ts:1888`; openai-python assigns per chunk).
        if let Nullable::Value(reason) = &choice.finish_reason {
            self.finish_reason = Some(reason.clone());
        }
        if let Omittable::Value(Nullable::Value(logprobs)) = &choice.logprobs {
            let state = self.logprobs.get_or_insert_with(LogprobsState::default);
            if let Nullable::Value(content) = &logprobs.content {
                state
                    .content
                    .get_or_insert_with(Vec::new)
                    .extend(content.iter().map(to_value_lossy));
            }
            if let Nullable::Value(refusal) = &logprobs.refusal {
                state
                    .refusal
                    .get_or_insert_with(Vec::new)
                    .extend(refusal.iter().map(to_value_lossy));
            }
        }
        for (key, value) in choice.extra().iter() {
            self.extra
                .entry(key.to_owned())
                .or_insert_with(|| value.clone());
        }

        let delta = &choice.delta;
        // Last announced role wins (`ChatCompletionStream.ts:1937`).
        if let Omittable::Value(role) = &delta.role {
            self.role = Some(role.clone());
        }
        if let Omittable::Value(Nullable::Value(text)) = &delta.content {
            match &mut self.content {
                Some(accumulated) => accumulated.push_str(text),
                None => self.content = Some(text.clone()),
            }
        }
        if let Omittable::Value(refusal) = &delta.refusal {
            match refusal {
                Nullable::Null => {
                    self.refusal.get_or_insert(Nullable::Null);
                }
                Nullable::Value(text) => match &mut self.refusal {
                    Some(Nullable::Value(accumulated)) => accumulated.push_str(text),
                    _ => self.refusal = Some(Nullable::Value(text.clone())),
                },
                // `Nullable` is `#[non_exhaustive]`; only the two pinned
                // states exist on the wire.
                _ => {}
            }
        }
        if let Omittable::Value(function_call) = &delta.function_call {
            let state = self
                .legacy_function_call
                .get_or_insert_with(LegacyFunctionCallState::default);
            if state.name.is_none() {
                if let Omittable::Value(name) = &function_call.name {
                    state.name = Some(name.clone());
                }
            }
            if let Omittable::Value(arguments) = &function_call.arguments {
                state.arguments.push_str(arguments.as_str());
            }
        }
        if let Omittable::Value(tool_calls) = &delta.tool_calls {
            self.tool_calls_seen = true;
            for call in tool_calls {
                self.tool_calls.entry(call.index).or_default().observe(call);
            }
        }
        merge_extra(&mut self.message_extra, delta.extra());
    }

    fn snapshot(&self, index: u32, passthrough: bool) -> Value {
        let mut choice = Map::new();
        choice.insert(
            String::from("finish_reason"),
            Value::String(String::from(
                self.finish_reason
                    .as_ref()
                    .map_or(ChatFinishReason::Stop.as_str(), |reason| reason.as_str()),
            )),
        );
        choice.insert(String::from("index"), Value::Number(Number::from(index)));
        choice.insert(
            String::from("message"),
            Value::Object(self.message(passthrough)),
        );
        choice.insert(String::from("logprobs"), self.logprobs_value());
        if passthrough {
            insert_first_seen(&mut choice, &self.extra);
        }
        Value::Object(choice)
    }

    fn message(&self, passthrough: bool) -> Map<String, Value> {
        let mut message = Map::new();
        message.insert(
            String::from("role"),
            Value::String(String::from(
                self.role
                    .as_ref()
                    .map_or(ChatRole::Assistant.as_str(), |role| role.as_str()),
            )),
        );
        message.insert(
            String::from("content"),
            match &self.content {
                Some(content) => Value::String(content.clone()),
                // Both SDKs leave the content unset (their loose `null`)
                // until the first text delta.
                None => Value::Null,
            },
        );
        if let Some(refusal) = &self.refusal {
            message.insert(String::from("refusal"), to_value_lossy(refusal));
        }
        if self.tool_calls_seen {
            let calls = self
                .tool_calls
                .values()
                .filter_map(|call| call.snapshot(passthrough))
                .collect::<Vec<_>>();
            message.insert(String::from("tool_calls"), Value::Array(calls));
        }
        if let Some(function_call) = &self.legacy_function_call {
            let mut legacy = Map::new();
            legacy.insert(
                String::from("name"),
                Value::String(function_call.name.clone().unwrap_or_default()),
            );
            legacy.insert(
                String::from("arguments"),
                Value::String(function_call.arguments.clone()),
            );
            message.insert(String::from("function_call"), Value::Object(legacy));
        }
        if passthrough {
            insert_first_seen(&mut message, &self.message_extra);
        }
        message
    }

    fn logprobs_value(&self) -> Value {
        let Some(state) = &self.logprobs else {
            // A chunk-level JSON `null` is skipped by both SDKs, so an
            // unobserved logprobs payload stays null on the snapshot.
            return Value::Null;
        };
        let mut logprobs = Map::new();
        logprobs.insert(String::from("content"), logprobs_list(&state.content));
        logprobs.insert(String::from("refusal"), logprobs_list(&state.refusal));
        Value::Object(logprobs)
    }
}

/// Per-tool-call fold state, addressed by the chunk `index`.
#[derive(Debug, Clone, Default)]
struct ToolCallState {
    id: Option<String>,
    kind: Option<ChatToolKind>,
    function_seen: bool,
    name: Option<String>,
    arguments: String,
    /// Unknown tool-call fields, deep-merged per `_deltas.py`; carries the
    /// untyped `custom` payload of custom-tool calls.
    extra: Map<String, Value>,
}

impl ToolCallState {
    fn observe(&mut self, call: &ChatToolCallChunk) {
        // The wire announces identity once; the first observed value wins.
        if self.id.is_none() {
            if let Omittable::Value(id) = &call.id {
                self.id = Some(id.clone());
            }
        }
        if self.kind.is_none() {
            if let Omittable::Value(kind) = &call.kind {
                self.kind = Some(kind.clone());
            }
        }
        if let Omittable::Value(function) = &call.function {
            self.function_seen = true;
            if self.name.is_none() {
                if let Omittable::Value(name) = &function.name {
                    self.name = Some(name.clone());
                }
            }
            if let Omittable::Value(arguments) = &function.arguments {
                self.arguments.push_str(arguments.as_str());
            }
        }
        merge_extra(&mut self.extra, call.extra());
    }

    fn snapshot(&self, passthrough: bool) -> Option<Value> {
        let mut call = Map::new();
        match &self.kind {
            Some(ChatToolKind::Custom) => {
                call.insert(String::from("type"), Value::String(String::from("custom")));
                call.insert(String::from("id"), self.identity());
                let (name, input) = self.custom_invocation();
                let mut custom = Map::new();
                custom.insert(String::from("name"), Value::String(name));
                custom.insert(String::from("input"), Value::String(input));
                call.insert(String::from("custom"), Value::Object(custom));
            }
            Some(ChatToolKind::Function) => self.insert_function(&mut call),
            // `ChatToolKind` is `#[non_exhaustive]`; an announced-but-unmodeled
            // kind — including `custom`-style future payloads — is retained
            // with its exact wire string and merged extra fields.
            Some(kind) => {
                call.insert(
                    String::from("type"),
                    Value::String(kind.as_str().to_owned()),
                );
                call.insert(String::from("id"), self.identity());
            }
            // A function payload identifies the call even before the `type`
            // announcement; a bare index with neither stays unrepresented.
            None if self.function_seen => self.insert_function(&mut call),
            None => return None,
        }
        if passthrough {
            insert_first_seen(&mut call, &self.extra);
        }
        Some(Value::Object(call))
    }

    /// openai-node seeds the function payload as
    /// `{ name: functionName ?? "", arguments: "" }`
    /// (`ChatCompletionStream.ts:2036`).
    fn insert_function(&self, call: &mut Map<String, Value>) {
        call.insert(
            String::from("type"),
            Value::String(String::from("function")),
        );
        call.insert(String::from("id"), self.identity());
        let mut function = Map::new();
        function.insert(
            String::from("name"),
            Value::String(self.name.clone().unwrap_or_default()),
        );
        function.insert(
            String::from("arguments"),
            Value::String(self.arguments.clone()),
        );
        call.insert(String::from("function"), Value::Object(function));
    }

    fn identity(&self) -> Value {
        Value::String(self.id.clone().unwrap_or_default())
    }

    /// Reads the folded `custom` payload from the merged extra fields.
    ///
    /// openai-node seeds `{ name: custom.name ?? "", input: "" }`
    /// (`ChatCompletionStream.ts:2027`) and the `_deltas.py` object merge
    /// accumulates `input` across chunks.
    fn custom_invocation(&self) -> (String, String) {
        self.extra
            .get("custom")
            .and_then(Value::as_object)
            .map(|custom| {
                (
                    custom
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    custom
                        .get("input")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                )
            })
            .unwrap_or_default()
    }
}

/// Legacy single `function_call` fold state.
#[derive(Debug, Clone, Default)]
struct LegacyFunctionCallState {
    name: Option<String>,
    arguments: String,
}

/// Extended token-logprob lists, retained in decoded form.
#[derive(Debug, Clone, Default)]
struct LogprobsState {
    content: Option<Vec<Value>>,
    refusal: Option<Vec<Value>>,
}

fn logprobs_list(list: &Option<Vec<Value>>) -> Value {
    match list {
        Some(items) => Value::Array(items.clone()),
        None => Value::Null,
    }
}

/// Inserts first-observed unknown fields without replacing computed ones.
fn insert_first_seen(target: &mut Map<String, Value>, extra: &Map<String, Value>) {
    for (key, value) in extra {
        target.entry(key.clone()).or_insert_with(|| value.clone());
    }
}

/// Deep-merges unknown chunk fields into an accumulated map per
/// openai-python's `accumulate_delta` (`lib/streaming/_deltas.py:6`).
fn merge_extra(target: &mut Map<String, Value>, extra: &openai_rs_types::ExtraFields) {
    let mut delta = Map::new();
    for (key, value) in extra.iter() {
        delta.insert(key.to_owned(), value.clone());
    }
    merge_delta_object(target, delta);
}

/// Merges a delta object into an accumulated object, mirroring
/// `accumulate_delta`'s per-key rules (`_deltas.py:7-24`): new and previously
/// null keys are replaced, `index` and `type` keys are replaced because they
/// key array entries and discriminated unions, and everything else recurses.
fn merge_delta_object(acc: &mut Map<String, Value>, delta: Map<String, Value>) {
    for (key, delta_value) in delta {
        let replace = key == "index"
            || key == "type"
            || !acc.contains_key(&key)
            || acc.get(&key).is_some_and(Value::is_null);
        if replace {
            acc.insert(key, delta_value);
            continue;
        }
        if let Some(slot) = acc.get_mut(&key) {
            merge_delta_value(slot, delta_value);
        }
    }
}

/// Merges one delta value into its accumulated slot per `_deltas.py:27-62`.
fn merge_delta_value(acc: &mut Value, delta: Value) {
    // A previously accumulated JSON null is replaced wholesale (`_deltas.py:13`).
    if acc.is_null() {
        *acc = delta;
        return;
    }
    match (acc, delta) {
        (Value::Object(acc_map), Value::Object(delta_map)) => {
            merge_delta_object(acc_map, delta_map);
        }
        (Value::String(accumulated), Value::String(text)) => accumulated.push_str(&text),
        (Value::Number(accumulated), Value::Number(addend)) => {
            if let Some(Value::Number(sum)) = sum_numbers(accumulated, &addend) {
                *accumulated = sum;
            }
        }
        (Value::Array(accumulated), Value::Array(entries)) => {
            merge_delta_array(accumulated, entries);
        }
        // Kind mismatches keep the accumulated value (`_deltas.py:62`).
        _ => {}
    }
}

fn merge_delta_array(acc: &mut Vec<Value>, delta: Vec<Value>) {
    // Lists of scalar entries only ever gain elements; lists of objects merge
    // their entries by the `index` key (`_deltas.py:34-60`).
    let scalars_only = acc
        .iter()
        .all(|item| matches!(item, Value::String(_) | Value::Number(_) | Value::Bool(_)));
    if scalars_only {
        acc.extend(delta);
        return;
    }
    for entry in delta {
        // openai-python raises on non-object entries and missing or
        // non-integer `index` keys (`_deltas.py:41-50`); the fold skips them.
        let Value::Object(map) = entry else {
            continue;
        };
        let Some(Value::Number(index)) = map.get("index") else {
            continue;
        };
        let Some(index) = index.as_u64() else {
            continue;
        };
        match acc.get_mut(index as usize) {
            Some(slot) => merge_delta_value(slot, Value::Object(map)),
            // openai-python inserts at the position; for the monotone indexes
            // the wire produces this is an append.
            None => acc.push(Value::Object(map)),
        }
    }
}

/// Adds two JSON numbers, mirroring `_deltas.py:29-30`.
///
/// Unsigned and signed pairs add exactly; everything else falls back to
/// floating point, where an unrepresentable sum keeps the accumulated value.
fn sum_numbers(acc: &Number, addend: &Number) -> Option<Value> {
    if let (Some(a), Some(b)) = (acc.as_u64(), addend.as_u64()) {
        return a
            .checked_add(b)
            .map(Value::from)
            .or_else(|| Number::from_f64(a as f64 + b as f64).map(Value::Number));
    }
    if let (Some(a), Some(b)) = (acc.as_i64(), addend.as_i64()) {
        return b
            .checked_add(a)
            .map(Value::from)
            .or_else(|| Number::from_f64(a as f64 + b as f64).map(Value::Number));
    }
    match (acc.as_f64(), addend.as_f64()) {
        (Some(a), Some(b)) => Number::from_f64(a + b).map(Value::Number),
        _ => None,
    }
}

/// Serializes a typed payload that has no failing representation.
fn to_value_lossy<T: Serialize + ?Sized>(value: &T) -> Value {
    // These payloads are plain data (strings, numbers, nested objects), so a
    // failure is not representable; `null` is the defensive fallback.
    serde_json::to_value(value).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn chunk(body: Value) -> ChatCompletionChunk {
        let mut body = body;
        if body.get("object").is_none() {
            body["object"] = json!("chat.completion.chunk");
        }
        serde_json::from_value(body).expect("decode chunk")
    }

    fn content_text(text: &str, finish: Value) -> Value {
        json!({
            "id": "chatcmpl_1",
            "choices": [{
                "delta": {"content": text, "role": "assistant"},
                "finish_reason": finish,
                "index": 0
            }],
            "created": 1,
            "model": "gpt-4o",
        })
    }

    #[test]
    fn empty_accumulator_has_no_snapshot_and_refuses_to_finish() {
        let accumulator = ChatCompletionAccumulator::new();
        assert!(accumulator.snapshot().is_none());
        assert!(!accumulator.is_done());
        let error = accumulator.finish().expect_err("nothing was observed");
        assert!(matches!(error, Error::StreamProtocol { .. }), "{error:?}");
    }

    #[test]
    fn non_chunk_objects_are_ignored_like_the_python_weak_filter() {
        let mut accumulator = ChatCompletionAccumulator::new();
        let mut filtered = content_text("hello", Value::Null);
        filtered["object"] = json!("");
        accumulator.push(&chunk(filtered));
        assert!(accumulator.snapshot().is_none());
    }

    #[test]
    fn content_and_role_fold_and_finish_reason_completes() {
        let mut accumulator = ChatCompletionAccumulator::new();
        accumulator.push(&chunk(json!({
            "id": "chatcmpl_1",
            "choices": [{
                "delta": {"content": null, "role": "assistant"},
                "finish_reason": null,
                "index": 0
            }],
            "created": 1,
            "model": "gpt-4o",
        })));
        let snapshot = accumulator
            .snapshot()
            .expect("snapshot after the first chunk");
        assert_eq!(snapshot.choices[0].message.role.as_str(), "assistant");
        assert!(snapshot.choices[0].message.content.is_null());
        // The pinned non-streaming model has no null stop reason; the stand-in
        // reads as `stop` until the terminal chunk arrives.
        assert_eq!(snapshot.choices[0].finish_reason.as_str(), "stop");
        assert!(!accumulator.is_done());

        accumulator.push(&chunk(content_text("Hello,", Value::Null)));
        accumulator.push(&chunk(content_text(" world.", json!("length"))));
        assert!(accumulator.is_done());
        assert_eq!(
            accumulator
                .finish_reason(0)
                .map(|reason| reason.as_str().to_owned()),
            Some(String::from("length"))
        );
        let completion = accumulator.finish().expect("terminal completion");
        assert_eq!(completion.id, "chatcmpl_1");
        assert_eq!(completion.model.as_str(), "gpt-4o");
        assert_eq!(completion.created, 1);
        assert_eq!(completion.choices[0].finish_reason.as_str(), "length");
        assert_eq!(
            completion.choices[0].message.content,
            Nullable::Value(String::from("Hello, world."))
        );
    }

    #[test]
    fn refusal_deltas_concatenate_after_an_explicit_null() {
        let mut accumulator = ChatCompletionAccumulator::new();
        accumulator.push(&chunk(json!({
            "id": "chatcmpl_1",
            "choices": [{
                "delta": {"refusal": null, "role": "assistant"},
                "finish_reason": null,
                "index": 0
            }],
            "created": 1,
            "model": "gpt-4o",
        })));
        accumulator.push(&chunk(json!({
            "id": "chatcmpl_1",
            "choices": [{
                "delta": {"refusal": "I cannot "},
                "finish_reason": null,
                "index": 0
            }],
            "created": 1,
            "model": "gpt-4o",
        })));
        accumulator.push(&chunk(json!({
            "id": "chatcmpl_1",
            "choices": [{
                "delta": {"refusal": "help with that."},
                "finish_reason": json!("stop"),
                "index": 0
            }],
            "created": 1,
            "model": "gpt-4o",
        })));
        let completion = accumulator.finish().expect("terminal completion");
        assert_eq!(
            completion.choices[0].message.refusal,
            Omittable::Value(Nullable::Value(String::from("I cannot help with that.")))
        );
    }

    #[test]
    fn tool_calls_interleave_by_index_with_first_identity_and_concatenated_arguments() {
        let mut accumulator = ChatCompletionAccumulator::new();
        let base = json!({
            "id": "chatcmpl_1",
            "created": 1,
            "model": "gpt-4o",
        });
        let with_choice = |delta: Value, finish: Value| {
            let mut body = base.clone();
            body["choices"] = json!([{ "delta": delta, "finish_reason": finish, "index": 0 }]);
            chunk(body)
        };
        accumulator.push(&with_choice(json!({"role": "assistant"}), Value::Null));
        accumulator.push(&with_choice(
            json!({"tool_calls": [{
                "index": 0,
                "id": "call_1",
                "type": "function",
                "function": {"name": "get_weather", "arguments": "{\"city\":"}
            }]}),
            Value::Null,
        ));
        // The second tool call starts before the first finishes.
        accumulator.push(&with_choice(
            json!({"tool_calls": [{
                "index": 1,
                "id": "call_2",
                "type": "function",
                "function": {"name": "get_time", "arguments": "{\"tz\":"}
            }]}),
            Value::Null,
        ));
        accumulator.push(&with_choice(
            json!({"tool_calls": [{
                "index": 0,
                "function": {"arguments": "\"Paris\"}"}
            }]}),
            Value::Null,
        ));
        accumulator.push(&with_choice(
            json!({"tool_calls": [{
                "index": 1,
                "function": {"arguments": "\"UTC\"}"}
            }]}),
            json!("tool_calls"),
        ));
        let completion = accumulator.finish().expect("terminal completion");
        let message = &completion.choices[0].message;
        let calls = match &message.tool_calls {
            Omittable::Value(Nullable::Value(calls)) => calls,
            other => panic!("tool calls must accumulate, got {other:?}"),
        };
        assert_eq!(calls.len(), 2);
        assert_eq!(
            serde_json::to_value(calls).expect("encode tool calls"),
            json!([
                {
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\":\"Paris\"}"}
                },
                {
                    "id": "call_2",
                    "type": "function",
                    "function": {"name": "get_time", "arguments": "{\"tz\":\"UTC\"}"}
                }
            ])
        );
        let arguments: Value = serde_json::from_str(match &calls[0] {
            openai_rs_types::chat::ChatToolCall::Function(call) => call.function.arguments.as_str(),
            other => panic!("first call is a function call, got {other:?}"),
        })
        .expect("concatenated arguments parse");
        assert_eq!(arguments["city"], "Paris");
    }

    #[test]
    fn custom_tool_payload_riding_extra_fields_accumulates_input() {
        let mut accumulator = ChatCompletionAccumulator::new();
        accumulator.push(&chunk(json!({
            "id": "chatcmpl_1",
            "created": 1,
            "model": "gpt-4o",
            "choices": [{
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_custom",
                        "type": "custom",
                        "custom": {"name": "highlight", "input": "open "}
                    }]
                },
                "finish_reason": null,
                "index": 0
            }]
        })));
        accumulator.push(&chunk(json!({
            "id": "chatcmpl_1",
            "created": 1,
            "model": "gpt-4o",
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "custom": {"input": "ai"}
                    }]
                },
                "finish_reason": json!("tool_calls"),
                "index": 0
            }]
        })));
        let completion = accumulator.finish().expect("terminal completion");
        let calls = match &completion.choices[0].message.tool_calls {
            Omittable::Value(Nullable::Value(calls)) => calls,
            other => panic!("custom tool call must accumulate, got {other:?}"),
        };
        match &calls[0] {
            openai_rs_types::chat::ChatToolCall::Custom(call) => {
                assert_eq!(call.id, "call_custom");
                assert_eq!(call.custom.name, "highlight");
                assert_eq!(call.custom.input, "open ai");
            }
            other => panic!("expected a custom tool call, got {other:?}"),
        }
    }

    #[test]
    fn legacy_function_call_folds_name_once_and_concatenates_arguments() {
        let mut accumulator = ChatCompletionAccumulator::new();
        accumulator.push(&chunk(json!({
            "id": "chatcmpl_1",
            "created": 1,
            "model": "gpt-3.5-turbo",
            "choices": [{
                "delta": {"role": "assistant", "function_call": {"name": "get_weather", "arguments": "{\"city\":"}},
                "finish_reason": null,
                "index": 0
            }]
        })));
        accumulator.push(&chunk(json!({
            "id": "chatcmpl_1",
            "created": 1,
            "model": "gpt-3.5-turbo",
            "choices": [{
                "delta": {"function_call": {"arguments": "\"Oslo\"}"}},
                "finish_reason": json!("function_call"),
                "index": 0
            }]
        })));
        let completion = accumulator.finish().expect("terminal completion");
        match &completion.choices[0].message.function_call {
            Omittable::Value(Nullable::Value(call)) => {
                assert_eq!(call.name, "get_weather");
                assert_eq!(call.arguments.as_str(), "{\"city\":\"Oslo\"}");
            }
            other => panic!("legacy function call must fold, got {other:?}"),
        }
    }

    #[test]
    fn usage_only_final_chunk_is_captured_and_first_seen_fields_win() {
        let mut accumulator = ChatCompletionAccumulator::new();
        accumulator.push(&chunk(content_text("Hello", Value::Null)));
        // A later chunk repeats a different model and id; the first observed
        // values win, matching openai-python seeding from the first chunk.
        let mut echo = content_text(" there", json!("stop"));
        echo["id"] = json!("chatcmpl_2");
        echo["model"] = json!("gpt-4o-mini");
        accumulator.push(&chunk(echo));
        accumulator.push(&chunk(json!({
            "id": "chatcmpl_2",
            "choices": [],
            "created": 2,
            "model": "gpt-4o-mini",
            "usage": {"prompt_tokens": 9, "completion_tokens": 2, "total_tokens": 11}
        })));
        accumulator.mark_done();
        let completion = accumulator.finish().expect("terminal completion");
        assert_eq!(completion.id, "chatcmpl_1");
        assert_eq!(completion.model.as_str(), "gpt-4o");
        assert_eq!(completion.created, 1);
        assert_eq!(
            completion.choices[0].message.content,
            Nullable::Value(String::from("Hello there"))
        );
        match &completion.usage {
            Omittable::Value(usage) => {
                assert_eq!(
                    (
                        usage.prompt_tokens,
                        usage.completion_tokens,
                        usage.total_tokens
                    ),
                    (9, 2, 11)
                );
            }
            _ => panic!("the final usage chunk must be captured"),
        }
    }

    #[test]
    fn mark_done_alone_completes_a_finish_reason_less_fold() {
        let mut accumulator = ChatCompletionAccumulator::new();
        accumulator.push(&chunk(content_text("Hello", Value::Null)));
        assert!(!accumulator.is_done());
        accumulator.mark_done();
        assert!(accumulator.is_done());
        let completion = accumulator.finish().expect("terminal completion");
        assert_eq!(
            completion.choices[0].message.content,
            Nullable::Value(String::from("Hello"))
        );
    }

    #[test]
    fn multiple_choices_fold_by_index() {
        let mut accumulator = ChatCompletionAccumulator::new();
        for (index, text) in ["first", "second"].into_iter().enumerate() {
            accumulator.push(&chunk(json!({
                "id": "chatcmpl_1",
                "created": 1,
                "model": "gpt-4o",
                "choices": [{
                    "delta": {"content": text, "role": "assistant"},
                    "finish_reason": json!("stop"),
                    "index": index
                }]
            })));
        }
        let completion = accumulator.finish().expect("terminal completion");
        let contents = completion
            .choices
            .iter()
            .map(|choice| choice.message.content.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            contents,
            vec![
                Nullable::Value(String::from("first")),
                Nullable::Value(String::from("second"))
            ]
        );
        assert_eq!(completion.choices[1].index, 1);
    }

    #[test]
    fn logprobs_token_lists_extend_across_chunks() {
        let token = json!({
            "token": "a",
            "logprob": -0.1,
            "bytes": null,
            "top_logprobs": []
        });
        let mut accumulator = ChatCompletionAccumulator::new();
        for finish in [Value::Null, json!("stop")] {
            accumulator.push(&chunk(json!({
                "id": "chatcmpl_1",
                "created": 1,
                "model": "gpt-4o",
                "choices": [{
                    "delta": {"role": "assistant"},
                    "finish_reason": finish,
                    "index": 0,
                    "logprobs": {"content": [token], "refusal": null}
                }]
            })));
        }
        let completion = accumulator.finish().expect("terminal completion");
        match &completion.choices[0].logprobs {
            Nullable::Value(logprobs) => {
                let content = match &logprobs.content {
                    Nullable::Value(content) => content,
                    _ => panic!("content logprobs must accumulate"),
                };
                assert_eq!(content.len(), 2);
                assert!(logprobs.refusal.is_null());
            }
            _ => panic!("logprobs must accumulate"),
        }
    }

    #[test]
    fn tool_calls_indices_gap_and_arrival_reordering_sort_by_index() {
        // 17-B-1a: index 2 arrives before index 0 and index 1 never arrives;
        // the fold keys by the chunk `index` (not arrival order), so the
        // snapshot emits the surviving calls sorted by index with no
        // placeholder left behind for the missing slot.
        let mut accumulator = ChatCompletionAccumulator::new();
        let base = json!({
            "id": "chatcmpl_1",
            "created": 1,
            "model": "gpt-4o",
        });
        let with_calls = |calls: Value, finish: Value| {
            let mut body = base.clone();
            body["choices"] = json!([{
                "delta": {"role": "assistant", "tool_calls": calls},
                "finish_reason": finish,
                "index": 0
            }]);
            chunk(body)
        };
        // The highest index arrives first, before any lower one exists.
        accumulator.push(&with_calls(
            json!([{
                "index": 2,
                "id": "call_third",
                "type": "function",
                "function": {"name": "third", "arguments": "{\"c\":3}"}
            }]),
            Value::Null,
        ));
        let snapshot = accumulator
            .snapshot()
            .expect("mid-stream snapshot after the out-of-order chunk");
        let calls = match &snapshot.choices[0].message.tool_calls {
            Omittable::Value(Nullable::Value(calls)) => calls,
            other => panic!("tool calls must be present mid-stream, got {other:?}"),
        };
        assert_eq!(
            serde_json::to_value(calls).expect("encode mid-stream calls"),
            json!([{
                "id": "call_third",
                "type": "function",
                "function": {"name": "third", "arguments": "{\"c\":3}"}
            }]),
            "a lone index-2 call must still be emitted, ordered by its own index"
        );

        accumulator.push(&with_calls(
            json!([{
                "index": 0,
                "id": "call_first",
                "type": "function",
                "function": {"name": "first", "arguments": "{\"a\":1}"}
            }]),
            json!("tool_calls"),
        ));
        let completion = accumulator.finish().expect("terminal completion");
        let calls = match &completion.choices[0].message.tool_calls {
            Omittable::Value(Nullable::Value(calls)) => calls,
            other => panic!("tool calls must accumulate, got {other:?}"),
        };
        // The arrival order was 2 then 0; the snapshot order is 0 then 2, and
        // the never-observed index 1 leaves no gap entry.
        let ids = calls
            .iter()
            .map(|call| match call {
                openai_rs_types::chat::ChatToolCall::Function(call) => call.id.as_str(),
                other => panic!("expected function calls, got {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["call_first", "call_third"]);
        assert_eq!(
            serde_json::to_value(calls).expect("encode tool calls"),
            json!([
                {
                    "id": "call_first",
                    "type": "function",
                    "function": {"name": "first", "arguments": "{\"a\":1}"}
                },
                {
                    "id": "call_third",
                    "type": "function",
                    "function": {"name": "third", "arguments": "{\"c\":3}"}
                }
            ])
        );
    }

    #[test]
    fn refusal_and_content_fold_independently_on_the_same_choice() {
        // 17-B-1b: both string lanes accumulate on one choice without the
        // refusal delta disturbing the content fold or vice versa.
        let mut accumulator = ChatCompletionAccumulator::new();
        let with_delta = |delta: Value, finish: Value| {
            chunk(json!({
                "id": "chatcmpl_1",
                "created": 1,
                "model": "gpt-4o",
                "choices": [{
                    "delta": delta,
                    "finish_reason": finish,
                    "index": 0
                }]
            }))
        };
        accumulator.push(&with_delta(
            json!({"role": "assistant", "content": "I can", "refusal": null}),
            Value::Null,
        ));
        accumulator.push(&with_delta(
            json!({"content": "not", "refusal": "I must"}),
            Value::Null,
        ));
        accumulator.push(&with_delta(
            json!({"content": " help.", "refusal": " decline."}),
            json!("stop"),
        ));
        let completion = accumulator.finish().expect("terminal completion");
        let message = &completion.choices[0].message;
        assert_eq!(
            message.content,
            Nullable::Value(String::from("I cannot help."))
        );
        assert_eq!(
            message.refusal,
            Omittable::Value(Nullable::Value(String::from("I must decline.")))
        );
    }

    #[test]
    fn an_empty_tool_calls_array_chunk_pins_the_current_behavior() {
        // 17-B-1c: an explicit `"tool_calls": []` sets `tool_calls_seen`, so
        // the snapshot carries an empty array. openai-python instead leaves
        // the field unset when no entry was observed (`_deltas.py:44-60`
        // assigns per entry), so this deliberately pins OUR divergence: the
        // empty array is observable on the decoded message.
        let mut accumulator = ChatCompletionAccumulator::new();
        accumulator.push(&chunk(json!({
            "id": "chatcmpl_1",
            "created": 1,
            "model": "gpt-4o",
            "choices": [{
                "delta": {"role": "assistant", "tool_calls": []},
                "finish_reason": json!("stop"),
                "index": 0
            }]
        })));
        let completion = accumulator.finish().expect("terminal completion");
        assert_eq!(
            completion.choices[0].message.tool_calls,
            Omittable::Value(Nullable::Value(Vec::new())),
            "an observed empty `tool_calls` array is pinned as an empty list, \
             unlike openai-python which leaves the field unset"
        );

        // A choice that never observed the field keeps it omitted.
        let mut untouched = ChatCompletionAccumulator::new();
        untouched.push(&chunk(content_text("Hello", json!("stop"))));
        let completion = untouched.finish().expect("terminal completion");
        assert_eq!(completion.choices[0].message.tool_calls, Omittable::Omitted);
    }

    #[test]
    fn empty_string_arguments_chunks_concatenate_harmlessly() {
        // 17-B-1d: some streams split JSON arguments across chunks where one
        // side of the split is the empty string; the fold is a plain
        // `push_str`, so the empty piece contributes nothing.
        let mut accumulator = ChatCompletionAccumulator::new();
        let with_arguments = |arguments: &str, finish: Value| {
            chunk(json!({
                "id": "chatcmpl_1",
                "created": 1,
                "model": "gpt-4o",
                "choices": [{
                    "delta": {"tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "echo", "arguments": arguments}
                    }]},
                    "finish_reason": finish,
                    "index": 0
                }]
            }))
        };
        accumulator.push(&with_arguments("", Value::Null));
        accumulator.push(&with_arguments("{\"a\"", Value::Null));
        accumulator.push(&with_arguments("", Value::Null));
        accumulator.push(&with_arguments(":1}", json!("tool_calls")));
        let completion = accumulator.finish().expect("terminal completion");
        let calls = match &completion.choices[0].message.tool_calls {
            Omittable::Value(Nullable::Value(calls)) => calls,
            other => panic!("tool calls must accumulate, got {other:?}"),
        };
        match &calls[0] {
            openai_rs_types::chat::ChatToolCall::Function(call) => {
                assert_eq!(call.function.arguments.as_str(), "{\"a\":1}");
            }
            other => panic!("expected a function call, got {other:?}"),
        }
    }

    #[test]
    fn merge_delta_value_mirrors_the_python_fold_rules() {
        // Strings concatenate, numbers add, objects recurse, `type` replaces.
        let mut acc = json!({
            "text": "a",
            "count": 2,
            "nested": {"deep": "x", "type": "old"},
            "list": [1, 2],
            "kept": "same"
        });
        merge_delta_value(
            &mut acc,
            json!({
                "text": "b",
                "count": 3,
                "nested": {"deep": "y", "type": "new"},
                "list": [3],
                "kept": 7,
                "fresh": true
            }),
        );
        assert_eq!(
            acc,
            json!({
                "text": "ab",
                "count": 5,
                "nested": {"deep": "xy", "type": "new"},
                "list": [1, 2, 3],
                "kept": "same",
                "fresh": true
            })
        );

        // A null slot is replaced wholesale and mismatched kinds keep the
        // accumulated value (`_deltas.py:13`, `_deltas.py:62`).
        let mut acc = json!({"slot": null, "mismatch": [1]});
        merge_delta_value(&mut acc, json!({"slot": {"a": 1}, "mismatch": "text"}));
        assert_eq!(acc, json!({"slot": {"a": 1}, "mismatch": [1]}));
    }

    #[test]
    fn merge_delta_value_merges_object_lists_by_index() {
        let mut acc = json!([{"index": 0, "text": "a"}]);
        merge_delta_value(
            &mut acc,
            json!([{"index": 0, "text": "b"}, {"index": 1, "text": "c"}]),
        );
        assert_eq!(
            acc,
            json!([{"index": 0, "text": "ab"}, {"index": 1, "text": "c"}])
        );
    }

    #[test]
    fn unknown_top_level_fields_ride_the_snapshot_first_seen() {
        let mut accumulator = ChatCompletionAccumulator::new();
        let mut first = content_text("Hello", json!("stop"));
        first["future_field"] = json!({"t": "one"});
        accumulator.push(&chunk(first));
        let mut second = content_text("!", Value::Null);
        second["future_field"] = json!({"t": "two"});
        accumulator.push(&chunk(second));
        let completion = accumulator.finish().expect("terminal completion");
        assert_eq!(
            completion.extra().get("future_field"),
            Some(&json!({"t": "one"}))
        );
    }

    #[test]
    fn hostile_passthrough_fields_degrade_instead_of_failing_the_snapshot() {
        // `audio` is typed on the non-streaming message but absent from the
        // delta model, so a partial payload rides the delta extras and cannot
        // decode into the strict message model; the fold still yields a
        // snapshot without the passthrough fields.
        let mut accumulator = ChatCompletionAccumulator::new();
        let mut body = content_text("Hello", json!("stop"));
        body["choices"][0]["delta"]["audio"] = json!({"data": "half"});
        accumulator.push(&chunk(body));
        let completion = accumulator.finish().expect("terminal completion");
        assert_eq!(
            completion.choices[0].message.content,
            Nullable::Value(String::from("Hello"))
        );
    }
}
