//! Built-in action executors (T109) behind the core policy (T108/T011): the UI names a
//! result and an action by id; Lumen looks the result up among what it recently showed,
//! authorizes the request, runs the executor and records the use for ranking (ADR-023).
//! Payloads (paths, launch keys) never come from the UI.

use lumen_core::builtin::{
    COPY_PATH, COPY_SYMBOL, DESCRIPTORS, EXCLUDE_FOLDER, LAUNCH, OPEN, REVEAL, REVEAL_REPOSITORY,
};
use lumen_core::{
    ActionDescriptor, ActionGroup, ActionId, ActionRequest, Invocation, Payload, QueryId, ResultId,
    ResultItem,
};
use lumen_search::{ActionError, available, prepare};
use tauri::{AppHandle, Manager, Runtime};

use crate::dto::ActionDto;
use crate::{overlay, search, settings};

/// Actions Lumen can execute (one registry for every provider).
pub(crate) static REGISTRY: [ActionDescriptor; 7] = DESCRIPTORS;

/// Keyboard hint shown in the Action Panel (must match `features/root-search/keymap.ts`).
pub(crate) fn shortcut_hint(action: &ActionId, primary: bool) -> Option<&'static str> {
    if primary {
        Some("Enter")
    } else if *action == REVEAL {
        Some("Ctrl+Enter")
    } else {
        None
    }
}

pub(crate) fn group_name(group: ActionGroup) -> &'static str {
    match group {
        ActionGroup::Primary => "primary",
        ActionGroup::Common => "common",
        ActionGroup::Navigation => "navigation",
        ActionGroup::Advanced => "advanced",
        ActionGroup::Destructive => "destructive",
    }
}

pub(crate) fn parse_invocation(s: &str) -> Option<Invocation> {
    match s {
        "primary" => Some(Invocation::Primary),
        "panel" => Some(Invocation::ActionPanel),
        "shortcut" => Some(Invocation::Shortcut),
        _ => None,
    }
}

pub(crate) fn lookup<R: Runtime>(
    app: &AppHandle<R>,
    query: u64,
    result: &str,
) -> Result<(QueryId, String, ResultItem), String> {
    let query = QueryId::new(query).ok_or("invalid query id")?;
    let result = ResultId::new(result).map_err(|_| "invalid result id")?;
    let state = app.state::<search::Search>();
    let service = state.0.as_ref().ok_or("search unavailable")?;
    let (text, item) = service
        .lookup(query, &result)
        .ok_or_else(|| ActionError::UnknownResult.to_string())?;
    Ok((query, text, item))
}

/// The Action Panel entries for a result the user saw.
///
/// # Errors
/// Invalid ids or a result Lumen no longer remembers.
pub(crate) fn list<R: Runtime>(
    app: &AppHandle<R>,
    query: u64,
    result: &str,
) -> Result<Vec<ActionDto>, String> {
    let (_, _, item) = lookup(app, query, result)?;
    Ok(available(&item, &REGISTRY)
        .into_iter()
        .map(|d| {
            let primary = d.id == item.primary_action;
            ActionDto {
                id: d.id.as_str().to_owned(),
                title: d.title.to_string(),
                group: group_name(d.group),
                shortcut: shortcut_hint(&d.id, primary),
            }
        })
        .collect())
}

/// Authorizes and runs `action` on a result; on success records the use and hides the
/// overlay.
///
/// # Errors
/// A short reason (invalid/unknown ids, refused by policy, executor failure).
pub(crate) fn run<R: Runtime>(
    app: &AppHandle<R>,
    query: u64,
    result: &str,
    action: &str,
    invocation: &str,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let (query, text, item) = lookup(app, query, result)?;
    let request = ActionRequest {
        query,
        result: item.id.clone(),
        action: ActionId::new(action).map_err(|_| "invalid action id")?,
        invocation: parse_invocation(invocation).ok_or("invalid invocation")?,
        confirmed: false,
    };
    let (ctx, _) = prepare(request, &item, &REGISTRY).map_err(|e| e.to_string())?;
    let action = ctx.request().action.clone();
    execute(app, &action, &item.payload)?;
    crate::diag::record("action_ms", started.elapsed().as_secs_f64() * 1000.0);
    record_use(app, &item, &action, &text);
    overlay::hide(app);
    Ok(())
}

fn path_of(payload: &Payload) -> Result<&std::path::Path, String> {
    payload
        .local_path()
        .ok_or_else(|| "result has no local path".into())
}

fn execute<R: Runtime>(
    app: &AppHandle<R>,
    action: &ActionId,
    payload: &Payload,
) -> Result<(), String> {
    let err = |e: &dyn std::fmt::Display| e.to_string();
    if *action == OPEN {
        tauri_plugin_opener::open_path(path_of(payload)?, None::<&str>).map_err(|e| err(&e))
    } else if *action == LAUNCH {
        match payload {
            // `shell:AppsFolder\<id>`: ShellExecute resolves it like the Start menu does.
            Payload::ProviderKey(uri) => {
                tauri_plugin_opener::open_url(uri.as_ref(), None::<&str>).map_err(|e| err(&e))
            }
            Payload::Path(p) => {
                tauri_plugin_opener::open_path(p, None::<&str>).map_err(|e| err(&e))
            }
            _ => Err("nothing to launch".into()),
        }
    } else if *action == REVEAL {
        tauri_plugin_opener::reveal_item_in_dir(path_of(payload)?).map_err(|e| err(&e))
    } else if *action == COPY_PATH {
        let text = path_of(payload)?.to_string_lossy().into_owned();
        arboard::Clipboard::new()
            .and_then(|mut c| c.set_text(text))
            .map_err(|e| err(&e))
    } else if *action == COPY_SYMBOL {
        let Payload::Code(code) = payload else {
            return Err("result has no code symbol".into());
        };
        let symbol = code.symbol.as_ref().ok_or("result has no code symbol")?;
        arboard::Clipboard::new()
            .and_then(|mut c| c.set_text(symbol.clone()))
            .map_err(|e| err(&e))
    } else if *action == REVEAL_REPOSITORY {
        let Payload::Code(code) = payload else {
            return Err("result has no repository".into());
        };
        let repository = code.repository.as_ref().ok_or("result has no repository")?;
        tauri_plugin_opener::reveal_item_in_dir(repository).map_err(|e| err(&e))
    } else if *action == EXCLUDE_FOLDER {
        // T111: saved in the locations setting; the next pass (started now) removes its items.
        if crate::catalog::exclude_path(app, path_of(payload)?) {
            Ok(())
        } else {
            Err("already excluded, or locations are read-only".into())
        }
    } else {
        Err("no executor for this action".into())
    }
}

fn record_use<R: Runtime>(app: &AppHandle<R>, item: &ResultItem, action: &ActionId, text: &str) {
    let state = app.state::<settings::Settings>();
    let Ok(mut guard) = state.0.lock() else {
        return;
    };
    let Some(store) = guard.as_mut() else { return };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    if let Err(err) = lumen_catalog::record_action(store, &item.id, action, text, now) {
        eprintln!("lumen: could not record use: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_and_invocations() {
        assert_eq!(shortcut_hint(&OPEN, true), Some("Enter"));
        assert_eq!(shortcut_hint(&REVEAL, false), Some("Ctrl+Enter"));
        assert_eq!(shortcut_hint(&COPY_PATH, false), None);
        assert_eq!(parse_invocation("panel"), Some(Invocation::ActionPanel));
        assert_eq!(parse_invocation("other"), None);
        assert_eq!(group_name(ActionGroup::Navigation), "navigation");
    }

    #[test]
    fn executors_need_a_path() {
        assert!(path_of(&Payload::Text("x".into())).is_err());
        assert!(path_of(&Payload::Path("/tmp/x".into())).is_ok());
    }
}
