//! Shared cursor-page helpers for resource list streams.

use std::collections::HashSet;

use crate::Error;

/// Records an already-requested forward cursor so a later page cannot repeat it.
pub(crate) fn seed_seen(seen: &mut HashSet<String>, after: Option<&str>) {
    if let Some(cursor) = after.filter(|value| !value.is_empty()) {
        seen.insert(cursor.to_owned());
    }
}

/// Rejects a backward cursor on automatic forward pagination.
pub(crate) fn reject_before_cursor(has_before: bool, resource: &str) -> Result<(), Error> {
    if has_before {
        Err(Error::InvalidConfiguration(
            format!("automatic {resource} pagination does not accept a before cursor").into(),
        ))
    } else {
        Ok(())
    }
}

/// Advances one list page.
///
/// When the page advertises `has_more` but its `last_id` cursor is missing or
/// empty, the caller-supplied identifier of the page's final element is used
/// instead, mirroring openai-python's `data[-1].id` fallback. Only when both
/// are unavailable does pagination fail closed; empty and repeated cursors
/// always fail closed.
pub(crate) fn next_cursor(
    has_more: bool,
    last_id: Option<&str>,
    last_item_id: Option<&str>,
    seen: &mut HashSet<String>,
    resource: &str,
) -> Result<Option<String>, Error> {
    if !has_more {
        return Ok(None);
    }
    let cursor = last_id
        .filter(|cursor| !cursor.is_empty())
        .or_else(|| last_item_id.filter(|cursor| !cursor.is_empty()))
        .ok_or_else(|| {
            Error::InvalidConfiguration(
                format!(
                    "{resource} page advertises more results without a last_id or last item id"
                )
                .into(),
            )
        })?;
    if !seen.insert(cursor.to_owned()) {
        return Err(Error::InvalidConfiguration(
            format!("{resource} pagination returned a repeated cursor").into(),
        ));
    }
    Ok(Some(cursor.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{next_cursor, reject_before_cursor, seed_seen};
    use crate::Error;
    use std::collections::HashSet;

    #[test]
    fn next_cursor_stops_without_has_more() {
        let mut seen = HashSet::new();
        let next = next_cursor(false, Some("cursor_1"), None, &mut seen, "batch").expect("no more");
        assert_eq!(next, None);
        assert!(seen.is_empty());
    }

    #[test]
    fn next_cursor_rejects_missing_empty_and_repeated_ids() {
        let mut seen = HashSet::new();
        let missing = next_cursor(true, None, None, &mut seen, "batch").expect_err("missing");
        assert!(matches!(missing, Error::InvalidConfiguration(_)));

        let empty = next_cursor(true, Some(""), None, &mut seen, "batch").expect_err("empty");
        assert!(matches!(empty, Error::InvalidConfiguration(_)));

        let first = next_cursor(true, Some("cursor_1"), None, &mut seen, "batch").expect("first");
        assert_eq!(first.as_deref(), Some("cursor_1"));
        let repeated =
            next_cursor(true, Some("cursor_1"), None, &mut seen, "batch").expect_err("repeat");
        assert!(matches!(repeated, Error::InvalidConfiguration(_)));
    }

    #[test]
    fn next_cursor_falls_back_to_the_last_item_id() {
        // openai-python derives the next cursor from `data[-1].id`, so a page
        // with `has_more=true` and a null last_id still advances when the
        // caller can name its final element.
        let mut seen = HashSet::new();
        let next = next_cursor(true, None, Some("item_9"), &mut seen, "batch").expect("fallback");
        assert_eq!(next.as_deref(), Some("item_9"));

        let mut seen = HashSet::new();
        let next =
            next_cursor(true, Some(""), Some("item_9"), &mut seen, "batch").expect("fallback");
        assert_eq!(next.as_deref(), Some("item_9"));

        // An empty fallback cannot rescue a missing cursor, and the server
        // cursor wins when both are present.
        let mut seen = HashSet::new();
        let error = next_cursor(true, None, Some(""), &mut seen, "batch").expect_err("empty");
        assert!(matches!(error, Error::InvalidConfiguration(_)));

        let mut seen = HashSet::new();
        let next = next_cursor(true, Some("cursor_1"), Some("item_9"), &mut seen, "batch")
            .expect("server cursor preferred");
        assert_eq!(next.as_deref(), Some("cursor_1"));
    }

    #[test]
    fn seed_seen_ignores_empty_and_reject_before_is_resource_specific() {
        let mut seen = HashSet::new();
        seed_seen(&mut seen, Some(""));
        seed_seen(&mut seen, None);
        seed_seen(&mut seen, Some("after_1"));
        assert_eq!(seen.len(), 1);
        reject_before_cursor(false, "vector-store").expect("forward only");
        let error = reject_before_cursor(true, "vector-store").expect_err("before");
        assert!(matches!(error, Error::InvalidConfiguration(_)));
    }
}
