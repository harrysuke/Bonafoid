//! The IDE command system: a registry of named commands that can be invoked
//! from menus, keyboard shortcuts, or the command palette.

/// Logical grouping for commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCategory {
    File,
    Edit,
    View,
    Model,
    Code,
    Tools,
    Help,
}

impl CommandCategory {
    /// Display label for the category.
    pub fn label(self) -> &'static str {
        match self {
            CommandCategory::File => "File",
            CommandCategory::Edit => "Edit",
            CommandCategory::View => "View",
            CommandCategory::Model => "Model",
            CommandCategory::Code => "Code",
            CommandCategory::Tools => "Tools",
            CommandCategory::Help => "Help",
        }
    }
}

/// Static metadata describing a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandDescriptor {
    /// Stable identifier, e.g. `file.new`.
    pub id: &'static str,
    /// Human-readable label.
    pub label: &'static str,
    /// Grouping.
    pub category: CommandCategory,
    /// Optional keyboard shortcut hint.
    pub shortcut: Option<&'static str>,
}

/// A queue-backed registry of the IDE's commands.
///
/// UI code calls [`CommandRegistry::trigger`] to enqueue a command; the
/// application loop drains the queue with [`CommandRegistry::drain`] and
/// performs the corresponding state mutation.
#[derive(Debug, Default)]
pub struct CommandRegistry {
    commands: Vec<CommandDescriptor>,
    queue: Vec<&'static str>,
}

impl CommandRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            queue: Vec::new(),
        }
    }

    /// Create a registry pre-populated with the built-in commands.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        for descriptor in DEFAULTS {
            registry.register(*descriptor);
        }
        registry
    }

    /// Register a command. Returns `false` if the id was already registered.
    pub fn register(&mut self, descriptor: CommandDescriptor) -> bool {
        if self.commands.iter().any(|c| c.id == descriptor.id) {
            return false;
        }
        self.commands.push(descriptor);
        true
    }

    /// All registered commands.
    pub fn all(&self) -> &[CommandDescriptor] {
        &self.commands
    }

    /// Look up a command by id.
    pub fn find(&self, id: &str) -> Option<&CommandDescriptor> {
        self.commands.iter().find(|c| c.id == id)
    }

    /// Enqueue a command for execution. Unknown ids are ignored.
    pub fn trigger(&mut self, id: &str) {
        if let Some(descriptor) = self.find(id) {
            self.queue.push(descriptor.id);
        }
    }

    /// Drain the pending commands.
    pub fn drain(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.queue)
    }

    /// Whether any commands are pending.
    pub fn has_pending(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Case-insensitive search across command labels and ids.
    pub fn search(&self, query: &str) -> Vec<&CommandDescriptor> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return self.commands.iter().collect();
        }
        self.commands
            .iter()
            .filter(|c| {
                c.label.to_lowercase().contains(&query) || c.id.to_lowercase().contains(&query)
            })
            .collect()
    }
}

/// The built-in command set.
pub const DEFAULTS: &[CommandDescriptor] = &[
    CommandDescriptor {
        id: "file.new",
        label: "New Project",
        category: CommandCategory::File,
        shortcut: Some("Ctrl+N"),
    },
    CommandDescriptor {
        id: "file.open",
        label: "Open Project…",
        category: CommandCategory::File,
        shortcut: Some("Ctrl+O"),
    },
    CommandDescriptor {
        id: "file.save",
        label: "Save Project",
        category: CommandCategory::File,
        shortcut: Some("Ctrl+S"),
    },
    CommandDescriptor {
        id: "file.close",
        label: "Close Project",
        category: CommandCategory::File,
        shortcut: None,
    },
    CommandDescriptor {
        id: "view.project",
        label: "Go to Project",
        category: CommandCategory::View,
        shortcut: Some("Ctrl+1"),
    },
    CommandDescriptor {
        id: "view.forms",
        label: "Go to Forms",
        category: CommandCategory::View,
        shortcut: Some("Ctrl+2"),
    },
    CommandDescriptor {
        id: "view.workflows",
        label: "Go to Workflows",
        category: CommandCategory::View,
        shortcut: Some("Ctrl+3"),
    },
    CommandDescriptor {
        id: "view.entities",
        label: "Go to Entities",
        category: CommandCategory::View,
        shortcut: Some("Ctrl+4"),
    },
    CommandDescriptor {
        id: "view.pages",
        label: "Go to Pages",
        category: CommandCategory::View,
        shortcut: Some("Ctrl+5"),
    },
    CommandDescriptor {
        id: "view.themes",
        label: "Go to Themes",
        category: CommandCategory::View,
        shortcut: Some("Ctrl+6"),
    },
    CommandDescriptor {
        id: "view.codegen",
        label: "Go to Code Generation",
        category: CommandCategory::View,
        shortcut: Some("Ctrl+7"),
    },
    CommandDescriptor {
        id: "view.plugins",
        label: "Go to Plugins",
        category: CommandCategory::View,
        shortcut: Some("Ctrl+8"),
    },
    CommandDescriptor {
        id: "view.settings",
        label: "Go to Settings",
        category: CommandCategory::View,
        shortcut: Some("Ctrl+9"),
    },
    CommandDescriptor {
        id: "model.add_form",
        label: "Add Form",
        category: CommandCategory::Model,
        shortcut: None,
    },
    CommandDescriptor {
        id: "model.add_entity",
        label: "Add Entity",
        category: CommandCategory::Model,
        shortcut: None,
    },
    CommandDescriptor {
        id: "model.add_workflow",
        label: "Add Workflow",
        category: CommandCategory::Model,
        shortcut: None,
    },
    CommandDescriptor {
        id: "model.add_page",
        label: "Add Page",
        category: CommandCategory::Model,
        shortcut: None,
    },
    CommandDescriptor {
        id: "code.generate",
        label: "Generate Code",
        category: CommandCategory::Code,
        shortcut: Some("Ctrl+G"),
    },
    CommandDescriptor {
        id: "code.validate",
        label: "Validate Workflows",
        category: CommandCategory::Code,
        shortcut: None,
    },
    CommandDescriptor {
        id: "app.command_palette",
        label: "Command Palette",
        category: CommandCategory::View,
        shortcut: Some("Ctrl+P"),
    },
    CommandDescriptor {
        id: "app.toggle_theme",
        label: "Toggle Dark Mode",
        category: CommandCategory::View,
        shortcut: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_registered() {
        let registry = CommandRegistry::with_defaults();
        assert_eq!(registry.all().len(), DEFAULTS.len());
        assert!(registry.find("file.new").is_some());
        assert!(registry.find("nope").is_none());
    }

    #[test]
    fn duplicate_registration_rejected() {
        let mut registry = CommandRegistry::new();
        assert!(registry.register(DEFAULTS[0]));
        assert!(!registry.register(DEFAULTS[0]));
    }

    #[test]
    fn trigger_queues_known_command_only() {
        let mut registry = CommandRegistry::with_defaults();
        registry.trigger("file.new");
        registry.trigger("does.not.exist");
        assert!(registry.has_pending());
        assert_eq!(registry.drain(), vec!["file.new"]);
        assert!(!registry.has_pending());
    }

    #[test]
    fn search_matches_label_and_id() {
        let registry = CommandRegistry::with_defaults();
        let by_label = registry.search("generate");
        assert!(by_label.iter().any(|c| c.id == "code.generate"));

        let by_id = registry.search("view.workflows");
        assert_eq!(by_id.len(), 1);
        assert_eq!(by_id[0].id, "view.workflows");

        let empty = registry.search("");
        assert_eq!(empty.len(), DEFAULTS.len());
    }

    #[test]
    fn all_command_ids_are_unique() {
        use std::collections::HashSet;
        let ids: HashSet<&str> = DEFAULTS.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), DEFAULTS.len());
    }
}
