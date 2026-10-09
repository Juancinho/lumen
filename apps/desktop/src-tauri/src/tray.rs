//! Tray icon: the always-available way to reach a hidden, resident Lumen, and (T003) where
//! the keyboard shortcut, (T004) the window material and (T111) the indexed locations and
//! exclusions are chosen, (T202) content indexing is watched and paused, and (T210)
//! semantic search is downloaded or removed, until a settings window exists.

use lumen_catalog::LocationState;
use lumen_catalog::locations::BUILD_DIRS_RULE;
use lumen_indexer::DEV_NOISE_NAMES;
use lumen_windows::material::{Material, MaterialChoice, Plan, Reason};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Runtime};
use tauri_plugin_dialog::DialogExt;

use crate::provisioning::{self, Setup};
use crate::{catalog, indexing, material, overlay, shortcut};

const TRAY_ID: &str = "lumen";
const MENU_SHOW: &str = "show";
const MENU_QUIT: &str = "quit";
const SHORTCUT_PREFIX: &str = "shortcut:";
const MATERIAL_PREFIX: &str = "material:";
const LOC_ADD: &str = "loc-add";
const LOC_REMOVE: &str = "loc-remove:";
const EX_ADD: &str = "ex-add";
const EX_REMOVE: &str = "ex-remove:";
const EX_NAME_REMOVE: &str = "ex-name-remove:";
const EX_DEFAULT: &str = "ex-default:";
const LOC_CONTENT: &str = "loc-content:";
const INDEX_PAUSE: &str = "index-pause";
const INDEX_GPU: &str = "index-gpu";
const SEM_DOWNLOAD: &str = "semantic-download";
const SEM_CANCEL: &str = "semantic-cancel";
const SEM_REMOVE: &str = "semantic-remove";
const VISION_DOWNLOAD: &str = "vision-download";
const VISION_REMOVE: &str = "vision-remove";

/// The semantic-search submenu's live entries (T210).
struct SemanticItems<R: Runtime> {
    status: MenuItem<R>,
    download: MenuItem<R>,
    cancel: MenuItem<R>,
    remove: MenuItem<R>,
    vision_status: MenuItem<R>,
    vision_download: MenuItem<R>,
    vision_remove: MenuItem<R>,
}

/// The content-indexing submenu's live entries.
struct IndexingItems<R: Runtime> {
    status: MenuItem<R>,
    images: MenuItem<R>,
    pause: CheckMenuItem<R>,
    gpu: CheckMenuItem<R>,
    gpu_status: MenuItem<R>,
}

/// The two submenus rebuilt whenever locations, exclusions or their states change.
struct LocationMenus<R: Runtime> {
    locations: Submenu<R>,
    exclusions: Submenu<R>,
}

/// Menu label of a location with the state of its last pass.
pub(crate) fn location_text(path: &str, state: Option<LocationState>) -> String {
    match state {
        None | Some(LocationState::Ok) => path.to_owned(),
        Some(LocationState::NotAvailable) => format!("{path} (not available)"),
        Some(LocationState::Partial { unlisted: 1 }) => format!("{path} (1 folder unreadable)"),
        Some(LocationState::Partial { unlisted }) => {
            format!("{path} ({unlisted} folders unreadable)")
        }
    }
}

/// Menu label of a default exclusion rule.
pub(crate) fn default_rule_text(rule: &str) -> String {
    if rule == BUILD_DIRS_RULE {
        "Build folders next to projects (target, build, dist, bin, obj)".to_owned()
    } else {
        rule.to_owned()
    }
}

/// The shortcut menu entries, kept to update checks/labels.
struct ShortcutItems<R: Runtime>(Vec<(&'static str, CheckMenuItem<R>)>);

/// The material menu entries.
struct MaterialItems<R: Runtime>(Vec<(MaterialChoice, CheckMenuItem<R>)>);

fn material_name(m: Material) -> &'static str {
    match m {
        Material::Acrylic => "Acrylic",
        Material::Mica => "Mica",
        Material::Solid => "Solid",
    }
}

/// Menu label of a material choice; the chosen one says what is really used when the
/// system forced a fallback (and `Automatic` always names what it resolved to).
pub(crate) fn material_text(choice: MaterialChoice, chosen: bool, plan: Option<Plan>) -> String {
    let base = match choice {
        MaterialChoice::Auto => "Automatic",
        MaterialChoice::Acrylic => "Acrylic",
        MaterialChoice::Mica => "Mica",
        MaterialChoice::Solid => "Solid",
    };
    let Some(plan) = plan.filter(|_| chosen) else {
        return base.to_owned();
    };
    let why = match plan.reason {
        Reason::AsRequested => None,
        Reason::HighContrast => Some("high contrast is on"),
        Reason::TransparencyOff => Some("transparency effects are off"),
        Reason::UnsupportedBuild => Some("needs Windows 11 22H2"),
    };
    match (choice, why) {
        (_, Some(why)) => format!("{base} (using {}: {why})", material_name(plan.material)),
        (MaterialChoice::Auto, None) => format!("{base} ({})", material_name(plan.material)),
        (_, None) => base.to_owned(),
    }
}

/// Menu label of a shortcut choice.
pub(crate) fn choice_text(label: &str, available: bool) -> String {
    if available {
        label.to_owned()
    } else {
        format!("{label} (in use by another app)")
    }
}

/// Tray tooltip for the active shortcut.
pub(crate) fn tooltip(active: Option<&str>) -> String {
    match active {
        Some(label) => format!("Lumen ({label})"),
        None => "Lumen - no keyboard shortcut available; right-click to choose one".to_owned(),
    }
}

pub(crate) fn install<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, MENU_SHOW, "Show Lumen", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit Lumen", true, None::<&str>)?;
    let mut items = Vec::new();
    for label in shortcut::CHOICES {
        let item = CheckMenuItem::with_id(
            app,
            format!("{SHORTCUT_PREFIX}{label}"),
            label,
            true,
            false,
            None::<&str>,
        )?;
        items.push((label, item));
    }
    let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = items
        .iter()
        .map(|(_, i)| i as &dyn tauri::menu::IsMenuItem<R>)
        .collect();
    let shortcuts = Submenu::with_items(app, "Keyboard shortcut", true, &refs)?;
    let mut material_items = Vec::new();
    for choice in MaterialChoice::ALL {
        let item = CheckMenuItem::with_id(
            app,
            format!("{MATERIAL_PREFIX}{}", choice.as_str()),
            material_text(choice, false, None),
            true,
            false,
            None::<&str>,
        )?;
        material_items.push((choice, item));
    }
    let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = material_items
        .iter()
        .map(|(_, i)| i as &dyn tauri::menu::IsMenuItem<R>)
        .collect();
    let materials = Submenu::with_items(app, "Window material", true, &refs)?;
    let index_status = MenuItem::new(app, "Content indexing", false, None::<&str>)?;
    let image_status = MenuItem::new(app, "Image indexing", false, None::<&str>)?;
    let index_pause = CheckMenuItem::with_id(
        app,
        INDEX_PAUSE,
        "Pause indexing",
        true,
        false,
        None::<&str>,
    )?;
    let index_gpu = CheckMenuItem::with_id(
        app,
        INDEX_GPU,
        "Use dedicated GPU for text indexing",
        false,
        false,
        None::<&str>,
    )?;
    let gpu_status = MenuItem::new(app, "Checking for a dedicated GPU…", false, None::<&str>)?;
    let indexing_menu = Submenu::with_items(
        app,
        "Content indexing",
        true,
        &[
            &index_status,
            &image_status,
            &index_pause,
            &index_gpu,
            &gpu_status,
        ],
    )?;
    let sem_status = MenuItem::new(app, "Semantic search", false, None::<&str>)?;
    let sem_download = MenuItem::with_id(app, SEM_DOWNLOAD, "Download…", true, None::<&str>)?;
    let sem_cancel = MenuItem::with_id(app, SEM_CANCEL, "Cancel download", false, None::<&str>)?;
    let sem_remove = MenuItem::with_id(app, SEM_REMOVE, "Remove…", false, None::<&str>)?;
    let vision_status = MenuItem::new(app, "Image search not installed", false, None::<&str>)?;
    let vision_download = MenuItem::with_id(
        app,
        VISION_DOWNLOAD,
        "Download image search…",
        true,
        None::<&str>,
    )?;
    let vision_remove = MenuItem::with_id(
        app,
        VISION_REMOVE,
        "Remove image encoder…",
        false,
        None::<&str>,
    )?;
    let semantic_menu = Submenu::with_items(
        app,
        "Semantic search",
        true,
        &[
            &sem_status,
            &sem_download,
            &sem_cancel,
            &sem_remove,
            &vision_status,
            &vision_download,
            &vision_remove,
        ],
    )?;
    let locations = Submenu::new(app, "Indexed locations", true)?;
    let exclusions = Submenu::new(app, "Exclusions", true)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &separator2,
            &locations,
            &exclusions,
            &indexing_menu,
            &semantic_menu,
            &shortcuts,
            &materials,
            &separator,
            &quit,
        ],
    )?;
    app.manage(ShortcutItems(items));
    app.manage(MaterialItems(material_items));
    app.manage(LocationMenus {
        locations,
        exclusions,
    });
    app.manage(IndexingItems {
        status: index_status,
        images: image_status,
        pause: index_pause,
        gpu: index_gpu,
        gpu_status,
    });
    app.manage(SemanticItems {
        status: sem_status,
        download: sem_download,
        cancel: sem_cancel,
        remove: sem_remove,
        vision_status,
        vision_download,
        vision_remove,
    });

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            if id == MENU_SHOW {
                overlay::show(app);
            } else if id == MENU_QUIT {
                app.exit(0);
            } else if id == SEM_DOWNLOAD {
                provisioning::ask_download(app);
            } else if id == SEM_CANCEL {
                provisioning::cancel(app);
            } else if id == SEM_REMOVE {
                provisioning::ask_remove(app);
            } else if id == VISION_DOWNLOAD {
                provisioning::ask_vision_download(app);
            } else if id == VISION_REMOVE {
                provisioning::ask_vision_remove(app);
            } else if id == INDEX_PAUSE {
                let paused = app.state::<indexing::Indexing>().paused();
                indexing::set_paused(app, !paused);
            } else if id == INDEX_GPU {
                let enabled = app.state::<crate::gpu::Gpu>().enabled();
                crate::gpu::choose(app, !enabled);
            } else if let Some(label) = id.strip_prefix(SHORTCUT_PREFIX) {
                shortcut::choose(app, label);
            } else if let Some(choice) = id
                .strip_prefix(MATERIAL_PREFIX)
                .and_then(MaterialChoice::parse)
            {
                material::choose(app, choice);
            } else {
                location_event(app, id);
            }
        })
        .on_tray_icon_event(|tray, event| {
            // Left click always shows (never toggles): clicking the tray first blurs
            // the overlay, which already hides it.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                overlay::show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    refresh(app.handle());
    refresh_material(app.handle());
    refresh_locations(app.handle());
    refresh_indexing(app.handle());
    refresh_semantic(app.handle());
    Ok(())
}

/// Updates the semantic-search status line and which actions apply (no-op before the tray).
pub(crate) fn refresh_semantic<R: Runtime>(app: &AppHandle<R>) {
    let Some(items) = app.try_state::<SemanticItems<R>>() else {
        return;
    };
    let setup = provisioning::setup(app);
    let _ = items.status.set_text(provisioning::setup_text(&setup));
    let _ = items
        .download
        .set_enabled(matches!(setup, Setup::Missing { .. } | Setup::Failed(_)));
    let _ = items
        .download
        .set_text(if matches!(setup, Setup::Failed(_)) {
            "Resume download…"
        } else {
            "Download…"
        });
    let _ = items
        .cancel
        .set_enabled(matches!(setup, Setup::Downloading { .. }));
    let _ = items
        .remove
        .set_enabled(setup == Setup::Ready && provisioning::model_removable());
    let vision = provisioning::vision_dir().is_some();
    let _ = items.vision_status.set_text(if vision {
        "Image search installed · CPU indexing"
    } else {
        "Image search not installed (109 MB download)"
    });
    let downloading = provisioning::busy(app);
    let _ = items
        .vision_download
        .set_enabled(!vision && !downloading && setup != Setup::Unsupported);
    let _ = items
        .vision_remove
        .set_enabled(vision && !downloading && provisioning::vision_removable());
}

/// Updates the content-indexing status line and pause check (no-op before the tray).
pub(crate) fn refresh_indexing<R: Runtime>(app: &AppHandle<R>) {
    let (Some(items), Some(state)) = (
        app.try_state::<IndexingItems<R>>(),
        app.try_state::<indexing::Indexing>(),
    ) else {
        return;
    };
    let _ = items
        .status
        .set_text(indexing::status_text(&state.status()));
    let _ = items.pause.set_checked(state.paused());
    let _ = items
        .images
        .set_text(indexing::image_status_text(&state.status().images));
    if let Some(gpu) = app.try_state::<crate::gpu::Gpu>() {
        let status = gpu.status();
        let _ = items.gpu.set_checked(gpu.enabled());
        let _ = items.gpu.set_enabled(
            gpu.enabled()
                || !matches!(
                    status,
                    crate::gpu::Status::Discovering | crate::gpu::Status::NoDevice
                ),
        );
        let _ = items.gpu_status.set_text(crate::gpu::text(&status));
    }
}

fn pick_folder<R: Runtime>(
    app: &AppHandle<R>,
    title: &str,
    then: fn(&AppHandle<R>, &std::path::Path) -> bool,
) {
    let handle = app.clone();
    app.dialog()
        .file()
        .set_title(title)
        .pick_folder(move |picked| {
            if let Some(path) = picked.as_ref().and_then(|p| p.as_path()) {
                then(&handle, path);
            }
        });
}

fn location_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if id == LOC_ADD {
        pick_folder(app, "Add a folder or drive to Lumen", catalog::add_location);
    } else if id == EX_ADD {
        pick_folder(app, "Exclude a folder from Lumen", catalog::exclude_path);
    } else if let Some(path) = id.strip_prefix(LOC_REMOVE) {
        catalog::remove_location(app, path);
    } else if let Some(path) = id.strip_prefix(LOC_CONTENT) {
        let on = app
            .state::<catalog::Catalog>()
            .locations()
            .locations
            .iter()
            .any(|l| l.path == path && l.content != lumen_catalog::locations::CONTENT_NAMES);
        catalog::set_content(app, path, !on);
        refresh_locations(app); // the check toggled itself; show the real state
    } else if let Some(path) = id.strip_prefix(EX_REMOVE) {
        catalog::unexclude_path(app, path);
    } else if let Some(name) = id.strip_prefix(EX_NAME_REMOVE) {
        catalog::edit(app, |m| {
            let before = m.exclude_names.len();
            m.exclude_names.retain(|n| n != name);
            m.exclude_names.len() != before
        });
    } else if let Some(rule) = id.strip_prefix(EX_DEFAULT) {
        let enabled = app
            .state::<catalog::Catalog>()
            .locations()
            .default_enabled(rule);
        catalog::set_default(app, rule, !enabled);
        refresh_locations(app); // the check toggled itself; show the real state
    }
}

fn clear<R: Runtime>(menu: &Submenu<R>) -> tauri::Result<()> {
    for _ in 0..menu.items()?.len() {
        menu.remove_at(0)?;
    }
    Ok(())
}

/// Rebuilds the locations and exclusions submenus from the current model and states.
pub(crate) fn refresh_locations<R: Runtime>(app: &AppHandle<R>) {
    let (Some(menus), Some(state)) = (
        app.try_state::<LocationMenus<R>>(),
        app.try_state::<catalog::Catalog>(),
    ) else {
        return;
    };
    if let Err(err) = fill_locations(app, &menus, &state) {
        eprintln!("lumen: tray locations menu failed: {err}");
    }
}

fn fill_locations<R: Runtime>(
    app: &AppHandle<R>,
    menus: &LocationMenus<R>,
    state: &catalog::Catalog,
) -> tauri::Result<()> {
    let editable = !state.read_only();
    let model = state.locations();

    clear(&menus.locations)?;
    for view in state.views() {
        let content = CheckMenuItem::with_id(
            app,
            format!("{LOC_CONTENT}{}", view.path),
            "Index file contents",
            editable,
            model.indexes_content(&view.path),
            None::<&str>,
        )?;
        let remove = MenuItem::with_id(
            app,
            format!("{LOC_REMOVE}{}", view.path),
            "Remove from Lumen",
            editable,
            None::<&str>,
        )?;
        let entry = Submenu::with_items(
            app,
            location_text(&view.path, view.state),
            true,
            &[&content, &remove],
        )?;
        menus.locations.append(&entry)?;
    }
    if model.locations.is_empty() {
        let none = MenuItem::new(
            app,
            "No locations: only apps are found",
            false,
            None::<&str>,
        )?;
        menus.locations.append(&none)?;
    }
    menus
        .locations
        .append(&PredefinedMenuItem::separator(app)?)?;
    let add = MenuItem::with_id(app, LOC_ADD, "Add folder or drive…", editable, None::<&str>)?;
    menus.locations.append(&add)?;

    clear(&menus.exclusions)?;
    let label = MenuItem::new(app, "Left out by default", false, None::<&str>)?;
    menus.exclusions.append(&label)?;
    for rule in DEV_NOISE_NAMES.iter().copied().chain([BUILD_DIRS_RULE]) {
        let item = CheckMenuItem::with_id(
            app,
            format!("{EX_DEFAULT}{rule}"),
            default_rule_text(rule),
            editable,
            model.default_enabled(rule),
            None::<&str>,
        )?;
        menus.exclusions.append(&item)?;
    }
    menus
        .exclusions
        .append(&PredefinedMenuItem::separator(app)?)?;
    for path in &model.exclude_paths {
        let remove = MenuItem::with_id(
            app,
            format!("{EX_REMOVE}{path}"),
            "Include again",
            editable,
            None::<&str>,
        )?;
        menus
            .exclusions
            .append(&Submenu::with_items(app, path, true, &[&remove])?)?;
    }
    for name in &model.exclude_names {
        let remove = MenuItem::with_id(
            app,
            format!("{EX_NAME_REMOVE}{name}"),
            "Include again",
            editable,
            None::<&str>,
        )?;
        menus.exclusions.append(&Submenu::with_items(
            app,
            format!("Named \"{name}\""),
            true,
            &[&remove],
        )?)?;
    }
    let add = MenuItem::with_id(app, EX_ADD, "Exclude a folder…", editable, None::<&str>)?;
    menus.exclusions.append(&add)?;
    Ok(())
}

/// Updates the material check marks and labels (no-op before the tray exists).
pub(crate) fn refresh_material<R: Runtime>(app: &AppHandle<R>) {
    let (Some(items), Some(state)) = (
        app.try_state::<MaterialItems<R>>(),
        app.try_state::<material::State>(),
    ) else {
        return;
    };
    let chosen = state.choice();
    let plan = state.applied();
    for (choice, item) in &items.0 {
        let _ = item.set_checked(*choice == chosen);
        let _ = item.set_text(material_text(*choice, *choice == chosen, plan));
    }
}

/// Updates check marks, "(in use)" labels and the tooltip from the current state.
pub(crate) fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let active = app.state::<shortcut::Active>().label();
    if let Some(items) = app.try_state::<ShortcutItems<R>>() {
        for (label, item) in &items.0 {
            let _ = item.set_checked(active == Some(*label));
            let _ = item.set_text(choice_text(label, shortcut::available(app, label)));
        }
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tooltip(active)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_tooltips() {
        assert_eq!(choice_text("Ctrl+Space", true), "Ctrl+Space");
        assert_eq!(
            choice_text("Alt+Space", false),
            "Alt+Space (in use by another app)"
        );
        assert_eq!(tooltip(Some("Alt+Space")), "Lumen (Alt+Space)");
        assert!(tooltip(None).contains("right-click"));
    }

    #[test]
    fn location_labels_show_state() {
        assert_eq!(location_text("D:\\Proyectos", None), "D:\\Proyectos");
        assert_eq!(
            location_text("E:\\", Some(LocationState::NotAvailable)),
            "E:\\ (not available)"
        );
        assert_eq!(
            location_text("C:\\Data", Some(LocationState::Partial { unlisted: 3 })),
            "C:\\Data (3 folders unreadable)"
        );
        assert!(default_rule_text(BUILD_DIRS_RULE).contains("target"));
        assert_eq!(default_rule_text("node_modules"), "node_modules");
    }

    #[test]
    fn material_labels_explain_fallbacks() {
        use lumen_windows::material::Corners;
        let plan = |material, reason| {
            Some(Plan {
                material,
                corners: Corners::Round,
                reason,
            })
        };
        let acrylic = plan(Material::Acrylic, Reason::AsRequested);
        assert_eq!(
            material_text(MaterialChoice::Auto, true, acrylic),
            "Automatic (Acrylic)"
        );
        assert_eq!(
            material_text(MaterialChoice::Auto, false, acrylic),
            "Automatic"
        );
        assert_eq!(
            material_text(MaterialChoice::Acrylic, true, acrylic),
            "Acrylic"
        );
        assert_eq!(
            material_text(
                MaterialChoice::Mica,
                true,
                plan(Material::Solid, Reason::TransparencyOff)
            ),
            "Mica (using Solid: transparency effects are off)"
        );
        assert_eq!(
            material_text(
                MaterialChoice::Auto,
                true,
                plan(Material::Solid, Reason::UnsupportedBuild)
            ),
            "Automatic (using Solid: needs Windows 11 22H2)"
        );
        assert_eq!(material_text(MaterialChoice::Solid, true, None), "Solid");
    }
}
