// Copyright 2024 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cosmic::app::ContextDrawer;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, icon, settings, text};
use cosmic::{Apply, Element, Task, theme};
use cosmic_config::{ConfigGet, ConfigSet};
use cosmic_settings_config::shortcuts::{self, Action, Binding, Shortcuts};
use cosmic_settings_page as page;
use slab::Slab;
use slotmap::Key;
use std::borrow::Cow;
use std::{io, mem};
use std::str::FromStr;

#[derive(Clone, Debug)]
pub enum ShortcutMessage {
    AddAnotherKeybinding,
    ApplyReplace,
    CancelReplace,
    DeleteBinding(usize),
    DeleteShortcut(usize),
    EditBinding(usize, bool),
    InputBinding(usize, String),
    ResetBindings,
    ShowShortcut(usize, String),
    SubmitBinding(usize),
}

#[derive(Debug)]
pub struct ShortcutBinding {
    pub id: widget::Id,
    pub binding: Binding,
    pub input: String,
    pub is_default: bool,
    pub is_saved: bool,
}

impl ShortcutBinding {
    pub fn reset(&mut self) {
        self.input = if self.is_saved {
            self.binding.to_string()
        } else {
            String::new()
        };
    }
}

#[must_use]
#[derive(Debug)]
pub struct ShortcutModel {
    pub action: Action,
    pub bindings: Slab<ShortcutBinding>,
    pub description: String,
    pub modified: u16,
}

impl ShortcutModel {
    pub fn new(defaults: &Shortcuts, shortcuts: &Shortcuts, action: Action) -> Self {
        let (bindings, modified) =
            shortcuts
                .shortcuts(&action)
                .fold((Slab::new(), 0), |(mut slab, modified), binding| {
                    let is_default = defaults.0.get(binding) == Some(&action);

                    slab.insert(ShortcutBinding {
                        id: widget::Id::unique(),
                        binding: binding.clone(),
                        input: String::new(),
                        is_default,
                        is_saved: true,
                    });

                    (slab, if is_default { modified } else { modified + 1 })
                });

        let mut localized_description = super::localize_action(&action);
        if let Action::Spawn(_) = &action {
            localized_description = bindings
                .iter()
                .map(|(_, shortcut)| super::localize_custom_action(&action, &shortcut.binding))
                .take(1)
                .collect();
        }

        Self {
            description: localized_description,
            modified: defaults.0.iter().filter(|(_, a)| **a == action).fold(
                modified,
                |modified, (binding, _)| {
                    if bindings.iter().any(|(_, model)| model.binding == *binding) {
                        modified
                    } else {
                        modified + 1
                    }
                },
            ),
            action,
            bindings,
        }
    }
}

#[must_use]
pub struct Model {
    pub entity: page::Entity,
    pub add_keybindings_button_id: cosmic::widget::Id,
    pub defaults: Shortcuts,
    pub editing: Option<usize>,
    pub replace_dialog: Option<(usize, Binding, Action, String)>,
    pub shortcut_models: Slab<ShortcutModel>,
    pub shortcut_context: Option<usize>,
    pub shortcut_title: String,
    pub config: cosmic_config::Config,
    pub custom: bool,
    pub actions: fn(&Shortcuts, &Shortcuts) -> Slab<ShortcutModel>,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            entity: page::Entity::null(),
            add_keybindings_button_id: widget::Id::unique(),
            defaults: Shortcuts::default(),
            editing: None,
            replace_dialog: None,
            shortcut_models: Slab::new(),
            shortcut_context: None,
            shortcut_title: String::new(),
            config: shortcuts::context().unwrap(),
            custom: false,
            actions: |_, _| Slab::new(),
        }
    }
}

impl Model {
    pub fn actions(mut self, actions: fn(&Shortcuts, &Shortcuts) -> Slab<ShortcutModel>) -> Self {
        self.actions = actions;
        self
    }

    pub fn custom(mut self) -> Self {
        self.custom = true;
        self
    }

    /// Adds a new binding to the shortcuts config
    pub(super) fn config_add(&self, action: Action, binding: Binding) {
        let mut shortcuts = self.shortcuts_config();
        shortcuts.0.insert(binding, action);
        self.shortcuts_config_set(shortcuts);
    }

    /// Check if a binding is already set
    pub(super) fn config_contains(&self, binding: &Binding) -> Option<Action> {
        self.shortcuts_system_config()
            .0
            .get(binding)
            .cloned()
            .filter(|action| *action != Action::Disable)
    }

    /// Removes a binding from the shortcuts config
    pub(super) fn config_remove(&self, binding: &Binding) {
        let mut shortcuts = self.shortcuts_config();
        shortcuts.0.retain(|b, _| b != binding);
        self.shortcuts_config_set(shortcuts);
    }

    pub(super) fn context_drawer(
        &self,
        apply: fn(ShortcutMessage) -> crate::pages::Message,
    ) -> Option<ContextDrawer<'_, crate::pages::Message>> {
        self.shortcut_context.as_ref().map(|id| {
            cosmic::app::context_drawer(
                context_drawer(
                    &self.shortcut_title,
                    &self.shortcut_models,
                    self.editing,
                    self.add_keybindings_button_id.clone(),
                    *id,
                    self.custom,
                )
                .map(apply),
                crate::pages::Message::CloseContextDrawer,
            )
        })
    }

    pub(super) fn dialog(&self) -> Option<Element<'_, ShortcutMessage>> {
        if let Some(&(id, _, _, ref action)) = self.replace_dialog.as_ref() {
            if let Some(short_id) = self.shortcut_context {
                if let Some(model) = self.shortcut_models.get(short_id) {
                    if let Some(shortcut) = model.bindings.get(id) {
                        let primary_action = button::suggested(fl!("replace"))
                            .on_press(ShortcutMessage::ApplyReplace);

                        let secondary_action = button::standard(fl!("cancel"))
                            .on_press(ShortcutMessage::CancelReplace);

                        let dialog = widget::dialog()
                            .title(fl!("replace-shortcut-dialog"))
                            .icon(icon::from_name("dialog-warning").size(64))
                            .body(fl!(
                                "replace-shortcut-dialog",
                                "desc",
                                shortcut = shortcut.input.clone(),
                                name = shortcut
                                    .binding
                                    .description
                                    .as_ref()
                                    .unwrap_or(action)
                                    .to_owned()
                            ))
                            .primary_action(primary_action)
                            .secondary_action(secondary_action);

                        return Some(dialog.into());
                    }
                }
            }
        }

        None
    }

    pub(super) fn on_enter(&mut self) {
        let mut shortcuts = self.config.get::<Shortcuts>("defaults").unwrap_or_default();
        self.defaults = shortcuts.clone();

        if let Ok(custom) = self.config.get::<Shortcuts>("custom") {
            for (binding, action) in custom.0 {
                shortcuts.0.remove(&binding);
                shortcuts.0.insert(binding, action);
            }
        }

        self.shortcut_models = (self.actions)(&self.defaults, &shortcuts);
        self.shortcut_context = None;
        self.editing = None;
    }

    pub(super) fn on_context_drawer_close(&mut self) {
        if let Some(short_id) = self.shortcut_context.take() {
            if let Some(model) = self.shortcut_models.get_mut(short_id) {
                if let Some(remove_id) = model
                    .bindings
                    .iter()
                    .find(|(_, binding)| !binding.is_saved)
                    .map(|(id, _)| id)
                {
                    model.bindings.remove(remove_id);
                }
            }
        }

        self.editing = None;
    }

    pub(super) fn on_clear(&mut self) {
        self.shortcut_models.clear();
        self.shortcut_models.shrink_to_fit();
    }

    /// Gets the custom configuration for keyboard shortcuts.
    pub(super) fn shortcuts_config(&self) -> Shortcuts {
        match self.config.get::<Shortcuts>("custom") {
            Ok(shortcuts) => shortcuts,
            Err(cosmic_config::Error::GetKey(_, why)) if why.kind() == io::ErrorKind::NotFound => {
                Shortcuts::default()
            }
            Err(why) => {
                tracing::error!(?why, "unable to get the current shortcuts config");
                Shortcuts::default()
            }
        }
    }

    /// Gets the system configuration for keyboard shortcuts.
    pub(super) fn shortcuts_system_config(&self) -> Shortcuts {
        let mut shortcuts = self.config.get::<Shortcuts>("defaults").unwrap_or_default();

        if let Ok(custom) = self.config.get::<Shortcuts>("custom") {
            shortcuts.0.extend(custom.0);
        }

        shortcuts
    }

    /// Writes a new configuration to the keyboard shortcuts config file.
    pub(super) fn shortcuts_config_set(&self, shortcuts: Shortcuts) {
        if let Err(why) = self.config.set("custom", shortcuts) {
            tracing::error!(?why, "failed to write shortcuts config");
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn update(&mut self, message: ShortcutMessage) -> Task<crate::app::Message> {
        match message {
            ShortcutMessage::AddAnotherKeybinding => {
                if let Some(short_id) = self.shortcut_context {
                    if let Some(model) = self.shortcut_models.get_mut(short_id) {
                        // If an empty entry exists, focus it instead of creating a new input.
                        for (binding_id, shortcut) in &mut model.bindings {
                            if shortcut.binding.is_set()
                                || Binding::from_str(&shortcut.input).is_ok()
                            {
                                continue;
                            }

                            self.editing = Some(binding_id);
                            shortcut.input.clear();

                            return widget::text_input::focus(shortcut.id.clone());
                        }

                        // Create a new input and focus it.
                        let id = widget::Id::unique();
                        self.editing = Some(model.bindings.insert(ShortcutBinding {
                            id: id.clone(),
                            binding: Binding::default(),
                            input: String::new(),
                            is_default: false,
                            is_saved: false,
                        }));

                        return widget::text_input::focus(id);
                    }
                }
            }

            ShortcutMessage::ApplyReplace => {
                if let Some((id, new_binding, ..)) = self.replace_dialog.take() {
                    if let Some(short_id) = self.shortcut_context {
                        // Remove conflicting bindings that are saved on disk.
                        self.config_remove(&new_binding);

                        // Clear any binding that matches this in the current model
                        for (_, model) in &mut self.shortcut_models {
                            if let Some(id) = model
                                .bindings
                                .iter()
                                .find(|(_, shortcut)| shortcut.binding == new_binding)
                                .map(|(id, _)| id)
                            {
                                model.bindings.remove(id);
                                break;
                            }
                        }

                        // Update the current model and save the binding to disk.
                        if let Some(model) = self.shortcut_models.get_mut(short_id) {
                            if let Some(shortcut) = model.bindings.get_mut(id) {
                                let prev_binding = shortcut.binding.clone();

                                shortcut.binding = new_binding.clone();
                                shortcut.input.clear();

                                if self.editing == Some(id) {
                                    self.editing = None;
                                }

                                let action = model.action.clone();
                                self.config_remove(&prev_binding);
                                self.config_add(action, new_binding);
                            }
                        }

                        self.on_enter();
                    }
                }
            }

            ShortcutMessage::CancelReplace => {
                if let Some(((id, _, _, _), short_id)) =
                    self.replace_dialog.take().zip(self.shortcut_context)
                {
                    if let Some(model) = self.shortcut_models.get_mut(short_id) {
                        if let Some(binding) = model.bindings.get_mut(id) {
                            binding.reset();
                            return cosmic::widget::text_input::focus(binding.id.clone());
                        }
                    }
                }
            }

            ShortcutMessage::DeleteBinding(id) => {
                if let Some(short_id) = self.shortcut_context {
                    if let Some(model) = self.shortcut_models.get_mut(short_id) {
                        let shortcut = model.bindings.remove(id);
                        if shortcut.is_default {
                            self.config_add(Action::Disable, shortcut.binding.clone());
                        } else {
                            self.config_remove(&shortcut.binding);
                        }
                    }
                }
            }

            ShortcutMessage::DeleteShortcut(id) => {
                let model = self.shortcut_models.remove(id);
                for (_, shortcut) in model.bindings {
                    self.config_remove(&shortcut.binding);
                }
            }

            ShortcutMessage::EditBinding(id, enable) => {
                if let Some(short_id) = self.shortcut_context {
                    if let Some(model) = self.shortcut_models.get_mut(short_id) {
                        if let Some(shortcut) = model.bindings.get_mut(id) {
                            if enable {
                                self.editing = Some(id);
                                shortcut.input = shortcut.binding.to_string();
                                return widget::text_input::select_all(shortcut.id.clone());
                            } else if self.editing == Some(id) {
                                self.editing = None;
                            }
                        }
                    }
                }
            }

            ShortcutMessage::InputBinding(id, text) => {
                if let Some(short_id) = self.shortcut_context {
                    if let Some(model) = self.shortcut_models.get_mut(short_id) {
                        if let Some(shortcut) = model.bindings.get_mut(id) {
                            shortcut.input = text;
                        }
                    }
                }
            }

            // Removes all bindings from the active shortcut context, and reloads the shortcuts model.
            ShortcutMessage::ResetBindings => {
                if let Some(short_id) = self.shortcut_context {
                    if let Some(model) = self.shortcut_models.get(short_id) {
                        for (_, shortcut) in &model.bindings {
                            self.config_remove(&shortcut.binding);
                        }

                        if let Ok(defaults) = self.config.get::<Shortcuts>("defaults") {
                            for (binding, action) in defaults.0 {
                                if action == model.action {
                                    self.config_remove(&binding);
                                }
                            }
                        }
                    }

                    self.on_enter();
                }
            }

            ShortcutMessage::ShowShortcut(id, description) => {
                self.shortcut_context = Some(id);
                self.shortcut_title = description;
                self.replace_dialog = None;

                let mut tasks = vec![cosmic::task::message(
                    crate::app::Message::OpenContextDrawer(self.entity),
                )];

                if let Some(model) = self.shortcut_models.get(0) {
                    if let Some(shortcut) = model.bindings.get(0) {
                        self.editing = Some(0);
                        tasks.push(widget::text_input::focus(shortcut.id.clone()));
                        tasks.push(widget::text_input::select_all(shortcut.id.clone()));
                    }
                }

                return Task::batch(tasks);
            }

            ShortcutMessage::SubmitBinding(id) => return self.submit_binding(id),
        }

        Task::none()
    }

    pub(super) fn view(&self) -> Element<ShortcutMessage> {
        self.shortcut_models
            .iter()
            .map(|(id, shortcut)| shortcut_item(self.custom, id, shortcut))
            .fold(widget::list_column(), widget::ListColumn::add)
            .into()
    }

    fn submit_binding(&mut self, id: usize) -> Task<crate::app::Message> {
        if let Some(short_id) = self.shortcut_context {
            let mut apply_binding = None;

            // Check for conflicts with the new binding.
            if let Some(model) = self.shortcut_models.get_mut(short_id) {
                if let Some(shortcut) = model.bindings.get_mut(id) {
                    if shortcut.input.is_empty() {
                        return Task::none();
                    }

                    match Binding::from_str(&shortcut.input) {
                        Ok(new_binding) => {
                            if shortcut.binding == new_binding {
                                return Task::none();
                            }

                            if !new_binding.is_set() {
                                shortcut.input.clear();
                                return Task::none();
                            }

                            if let Some(action) = self.config_contains(&new_binding) {
                                let action_str = if let Action::Spawn(_) = &action {
                                    super::localize_custom_action(&action, &new_binding)
                                } else {
                                    super::localize_action(&action)
                                };
                                self.replace_dialog = Some((id, new_binding, action, action_str));
                                return Task::none();
                            }

                            apply_binding = Some(new_binding);
                        }

                        Err(why) => {
                            tracing::error!(why, "keybinding input invalid");
                            shortcut.reset();
                        }
                    }
                }
            }

            // Apply if no conflict was found.
            if let Some(new_binding) = apply_binding {
                if let Some(model) = self.shortcut_models.get_mut(short_id) {
                    if let Some(shortcut) = model.bindings.get_mut(id) {
                        let prev_binding = mem::replace(&mut shortcut.binding, new_binding.clone());
                        
                        shortcut.is_saved = true;
                        shortcut.input.clear();

                        if self.editing == Some(id) {
                            self.editing = None;
                        }

                        let action = model.action.clone();
                        
                        if shortcut.is_default {
                            self.config_add(Action::Disable, prev_binding);
                        } else {
                            self.config_remove(&prev_binding);
                        }
                        self.config_add(action, new_binding);
                        return cosmic::widget::text_input::focus(
                            self.add_keybindings_button_id.clone(),
                        );
                    }
                }
            }
        }

        Task::none()
    }
}

fn context_drawer<'a>(
    title: &'a str,
    shortcuts: &'a Slab<ShortcutModel>,
    editing: Option<usize>,
    add_keybindings_id: widget::Id,
    id: usize,
    show_action: bool,
) -> Element<'a, ShortcutMessage> {
    let cosmic::cosmic_theme::Spacing {
        space_xxs,
        space_xs,
        space_l,
        ..
    } = theme::spacing();

    let model = &shortcuts[id];

    let action = show_action.then(|| {
        let description = if let Action::Spawn(task) = &model.action {
            Cow::Borrowed(task.as_str())
        } else {
            Cow::Owned(super::localize_action(&model.action))
        };

        text::body(description)
    });

    let bindings = model.bindings.iter().enumerate().fold(
        widget::list_column().spacing(space_xxs),
        |section, (_, (bind_id, shortcut))| {
            let editing = editing == Some(bind_id);
            let text: Cow<'_, str> = if !editing && shortcut.binding.is_set() {
                Cow::Owned(shortcut.binding.to_string())
            } else {
                Cow::Borrowed(&shortcut.input)
            };

            let input = widget::editable_input("", text, editing, move |enable| {
                ShortcutMessage::EditBinding(bind_id, enable)
            })
            .select_on_focus(true)
            .on_input(move |text| ShortcutMessage::InputBinding(bind_id, text))
            .on_unfocus(ShortcutMessage::SubmitBinding(bind_id))
            .on_submit(move |_| ShortcutMessage::SubmitBinding(bind_id))
            .padding([0, space_xs])
            .id(shortcut.id.clone())
            .into();

            let mut children = Vec::with_capacity(2);
            children.push(input);

            if shortcut.is_saved {
                let delete_button = widget::button::icon(icon::from_name("edit-delete-symbolic"))
                    .on_press(ShortcutMessage::DeleteBinding(bind_id))
                    .into();
                children.push(delete_button);
            }

            section.add(settings::item_row(children).align_y(Alignment::Center))
        },
    );

    let reset_keybinding_button = if model.modified == 0 || show_action {
        None
    } else {
        let button = widget::button::standard(fl!("reset-to-default"))
            .on_press(ShortcutMessage::ResetBindings);
        Some(button)
    };

    let add_keybinding_button = widget::button::standard(fl!("add-another-keybinding"))
        .id(add_keybindings_id)
        .on_press_maybe(if model.bindings.iter().any(|(_, b)| !b.is_saved) {
            None
        } else {
            Some(ShortcutMessage::AddAnotherKeybinding)
        });

    let button_container = widget::row::with_capacity(2)
        .push_maybe(reset_keybinding_button)
        .push(add_keybinding_button)
        .spacing(space_xs)
        .apply(widget::container)
        .width(Length::Fill)
        .align_x(Alignment::End);

    widget::column::with_capacity(if show_action { 4 } else { 3 })
        .push(widget::text::heading(title))
        .spacing(space_l)
        .push_maybe(action)
        .push(bindings)
        .push(button_container)
        .into()
}

/// Display a shortcut as a list item
fn shortcut_item(custom: bool, id: usize, data: &ShortcutModel) -> Element<ShortcutMessage> {
    #[derive(Copy, Clone, Debug)]
    enum LocalMessage {
        Remove,
        Show,
    }

    let bindings = data
        .bindings
        .iter()
        .take(3)
        .filter(|(_, shortcut)| shortcut.binding.is_set())
        .map(|(_, shortcut)| text::body(shortcut.binding.to_string()).into())
        .collect::<Vec<_>>();

    let shortcuts: Element<LocalMessage> = if bindings.is_empty() {
        text::body(fl!("disabled")).into()
    } else {
        widget::column::with_children(bindings)
            .align_x(Alignment::End)
            .into()
    };

    let modified = if data.modified == 0 {
        None
    } else {
        Some(text::body(fl!("modified", count = data.modified)))
    };

    let control = widget::row::with_capacity(4)
        .push_maybe(modified)
        .push(shortcuts)
        .push(icon::from_name("go-next-symbolic").size(16))
        .push_maybe(custom.then(|| {
            widget::button::icon(icon::from_name("edit-delete-symbolic"))
                .on_press(LocalMessage::Remove)
        }))
        .align_y(Alignment::Center)
        .spacing(8);

    settings::item::builder(&data.description)
        .flex_control(control)
        .spacing(16)
        .apply(widget::container)
        .class(theme::Container::List)
        .apply(widget::button::custom)
        .class(theme::Button::Transparent)
        .on_press(LocalMessage::Show)
        .apply(Element::from)
        .map(move |message| match message {
            LocalMessage::Show => ShortcutMessage::ShowShortcut(id, data.description.clone()),
            LocalMessage::Remove => ShortcutMessage::DeleteShortcut(id),
        })
}
