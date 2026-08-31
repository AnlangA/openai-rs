use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};
use tracing_core::span::Current;

thread_local! {
    static CURRENT_SPANS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CapturedSpan {
    pub name: String,
    pub fields: Vec<(String, String)>,
}

impl CapturedSpan {
    pub(crate) fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Default)]
struct Inner {
    spans: Vec<CapturedSpan>,
    events: Vec<(String, Vec<(String, String)>)>,
    by_id: HashMap<u64, usize>,
    metadata: HashMap<u64, &'static Metadata<'static>>,
}

#[derive(Clone)]
pub(crate) struct Capture {
    inner: Arc<Mutex<Inner>>,
    next_id: Arc<AtomicU64>,
}

impl Capture {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn spans(&self) -> Vec<CapturedSpan> {
        self.lock().spans.clone()
    }

    pub(crate) fn contains_text(&self, needle: &str) -> bool {
        let inner = self.lock();
        inner.spans.iter().any(|span| {
            span.fields
                .iter()
                .any(|(key, value)| key.contains(needle) || value.contains(needle))
        }) || inner.events.iter().any(|(_, fields)| {
            fields
                .iter()
                .any(|(key, value)| key.contains(needle) || value.contains(needle))
        })
    }

    pub(crate) fn events_contain(&self, message: &str) -> bool {
        self.lock().events.iter().any(|(_, fields)| {
            fields
                .iter()
                .any(|(key, value)| key == "message" && value == message)
        })
    }
}

struct FieldCollector<'a>(&'a mut Vec<(String, String)>);

impl Visit for FieldCollector<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.push((field.name().to_owned(), value.to_string()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.push((field.name().to_owned(), value.to_string()));
    }
}

impl Subscriber for Capture {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut fields = Vec::new();
        attributes.record(&mut FieldCollector(&mut fields));
        let mut inner = self.lock();
        let index = inner.spans.len();
        inner.spans.push(CapturedSpan {
            name: attributes.metadata().name().to_owned(),
            fields,
        });
        inner.by_id.insert(id, index);
        inner.metadata.insert(id, attributes.metadata());
        Id::from_u64(id)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let mut fields = Vec::new();
        values.record(&mut FieldCollector(&mut fields));
        let mut inner = self.lock();
        if let Some(index) = inner.by_id.get(&span.into_u64()).copied()
            && let Some(captured) = inner.spans.get_mut(index)
        {
            captured.fields.extend(fields);
        }
    }

    fn event(&self, event: &Event<'_>) {
        let mut fields = Vec::new();
        event.record(&mut FieldCollector(&mut fields));
        self.lock()
            .events
            .push((event.metadata().level().to_string(), fields));
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn enter(&self, span: &Id) {
        CURRENT_SPANS.with(|stack| stack.borrow_mut().push(span.into_u64()));
    }

    fn exit(&self, span: &Id) {
        CURRENT_SPANS.with(|stack| {
            let mut stack = stack.borrow_mut();
            if let Some(index) = stack.iter().rposition(|id| *id == span.into_u64()) {
                stack.remove(index);
            }
        });
    }

    fn current_span(&self) -> Current {
        CURRENT_SPANS.with(|stack| {
            let Some(id) = stack.borrow().last().copied() else {
                return Current::none();
            };
            match self.lock().metadata.get(&id).copied() {
                Some(metadata) => Current::new(Id::from_u64(id), metadata),
                None => Current::none(),
            }
        })
    }
}
