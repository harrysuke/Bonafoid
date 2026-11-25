//! Domain-slice diffs.
//!
//! The plan chose domain maps over RFC 6902 JSON Patch (§12 decision 3): the
//! frontend applies a patch by assigning one property, and there is no patch
//! dependency on either side. The cost is that a one-field edit resends the
//! whole `entities` map — acceptable for projects of this size, and the reason
//! `project_updated` exists for very large ones.

use nocode_model::Project;
use serde_json::{json, Value};

/// One changed domain.
#[derive(Debug, Clone)]
pub struct DomainPatch {
    pub domain: &'static str,
    pub value: Value,
}

/// Domains a patch can carry, in the order they are emitted.
pub const DOMAINS: [&str; 5] = ["entities", "forms", "pages", "themes", "workflows"];

/// Compare two snapshots and return the domains that changed.
pub fn diff(before: &Project, after: &Project) -> Vec<DomainPatch> {
    let mut patches = Vec::new();
    for domain in DOMAINS {
        let (old, new) = match domain {
            "entities" => (json!(before.entities), json!(after.entities)),
            "forms" => (json!(before.forms), json!(after.forms)),
            "pages" => (json!(before.pages), json!(after.pages)),
            "themes" => (json!(before.themes), json!(after.themes)),
            "workflows" => (json!(before.workflows), json!(after.workflows)),
            _ => unreachable!("DOMAINS and this match must stay in step"),
        };
        if old != new {
            patches.push(DomainPatch { domain, value: new });
        }
    }
    patches
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocode_model::{Entity, Form};

    #[test]
    fn an_unchanged_project_produces_no_patches() {
        let project = Project::new("App");
        assert!(diff(&project, &project).is_empty());
    }

    #[test]
    fn adding_an_entity_patches_only_entities() {
        let before = Project::new("App");
        let mut after = before.clone();
        let entity = Entity::new("Expense");
        after.entities.insert(entity.id.clone(), entity);

        let patches = diff(&before, &after);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].domain, "entities");
        assert_eq!(patches[0].value.as_object().unwrap().len(), 1);
    }

    #[test]
    fn several_domains_can_change_at_once() {
        let before = Project::new("App");
        let mut after = before.clone();
        let entity = Entity::new("Expense");
        after.entities.insert(entity.id.clone(), entity);
        let form = Form::new("Expense form");
        after.forms.insert(form.id.clone(), form);

        let domains: Vec<&str> = diff(&before, &after).iter().map(|p| p.domain).collect();
        assert_eq!(domains, vec!["entities", "forms"]);
    }
}
