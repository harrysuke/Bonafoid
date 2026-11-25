//! Per-object three-way merge of a project.
//!
//! A run loads a project, edits it, and saves. If somebody else saved in
//! between, the save is rejected — correctly, because the run's snapshot is
//! stale. Discarding the run at that point wastes the user's tokens and their
//! patience, so the orchestrator merges instead.
//!
//! This is **not** text merging. A project is a set of id-keyed objects, so the
//! merge rule per object id is three lines long and total:
//!
//! | base vs db | run vs base | result |
//! |------------|-------------|--------|
//! | equal      | any         | take the run's version (including a delete) |
//! | differs    | unchanged   | keep the database's version |
//! | differs    | changed     | **conflict** — both sides moved it |
//!
//! "Conflict" means the run must not guess. It reports the object ids and
//! saves nothing, which is the only outcome that cannot lose an edit.
//!
//! Plan §15.2.3.

use chrono::Utc;
use serde::Serialize;
use serde_json::{Map, Value};

use nocode_model::Project;

use crate::error::AgentError;

/// The five domains the agent can edit. Anything else in a project is
/// untouched by a rebase and stays as the database has it.
const DOMAINS: [&str; 5] = ["entities", "forms", "pages", "themes", "workflows"];

/// One object both sides changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict {
    pub domain: String,
    pub object_id: String,
}

impl Conflict {
    fn new(domain: &str, object_id: impl Into<String>) -> Self {
        Self {
            domain: domain.to_string(),
            object_id: object_id.into(),
        }
    }
}

/// The outcome of a merge.
#[derive(Debug)]
pub enum Rebase {
    /// Disjoint edits: this project contains both sides.
    Clean(Box<Project>),
    /// Overlapping edits: nothing may be written.
    Conflicts(Vec<Conflict>),
}

/// Merge the run's changes onto the current row.
///
/// * `base` — the project as the run loaded it;
/// * `theirs` — the run's final working copy;
/// * `ours` — the row as it is now.
pub fn rebase(base: &Project, theirs: &Project, ours: &Project) -> Result<Rebase, AgentError> {
    let mut conflicts = Vec::new();
    let mut merged = ours.clone();

    for domain in DOMAINS {
        let base_map = map_of(base, domain)?;
        let theirs_map = map_of(theirs, domain)?;
        let ours_map = map_of(ours, domain)?;
        let result = merge_domain(&base_map, &theirs_map, &ours_map, domain, &mut conflicts);
        let value = Value::Object(result);
        match domain {
            "entities" => merged.entities = serde_json::from_value(value)?,
            "forms" => merged.forms = serde_json::from_value(value)?,
            "pages" => merged.pages = serde_json::from_value(value)?,
            "themes" => merged.themes = serde_json::from_value(value)?,
            "workflows" => merged.workflows = serde_json::from_value(value)?,
            _ => unreachable!("DOMAINS and this match must stay in step"),
        }
    }

    // The active theme is a scalar pointer into `themes`, so it gets the same
    // three-way treatment. Without this, a run that switched the theme and a
    // human who switched it elsewhere would silently disagree with the map.
    if base.active_theme_id == ours.active_theme_id {
        merged.active_theme_id = theirs.active_theme_id.clone();
    } else if base.active_theme_id != theirs.active_theme_id
        && theirs.active_theme_id != ours.active_theme_id
    {
        conflicts.push(Conflict::new("themes", "active_theme"));
    }

    if conflicts.is_empty() {
        merged.updated_at = Utc::now();
        Ok(Rebase::Clean(Box::new(merged)))
    } else {
        Ok(Rebase::Conflicts(conflicts))
    }
}

/// Serialise one domain to an id-keyed map.
fn map_of(project: &Project, domain: &str) -> Result<Map<String, Value>, AgentError> {
    let value = match domain {
        "entities" => serde_json::to_value(&project.entities)?,
        "forms" => serde_json::to_value(&project.forms)?,
        "pages" => serde_json::to_value(&project.pages)?,
        "themes" => serde_json::to_value(&project.themes)?,
        "workflows" => serde_json::to_value(&project.workflows)?,
        _ => unreachable!("unknown domain `{domain}`"),
    };
    match value {
        Value::Object(map) => Ok(map),
        other => Err(AgentError::Internal(format!(
            "domain `{domain}` did not serialise to an object: {}",
            other
        ))),
    }
}

/// Merge one domain. Conflicting ids are pushed to `conflicts` and left as the
/// database has them, so a partial merge can never invent a value.
fn merge_domain(
    base: &Map<String, Value>,
    theirs: &Map<String, Value>,
    ours: &Map<String, Value>,
    domain: &str,
    conflicts: &mut Vec<Conflict>,
) -> Map<String, Value> {
    let mut merged = ours.clone();

    let mut ids: Vec<&String> = base
        .keys()
        .chain(theirs.keys())
        .chain(ours.keys())
        .collect();
    ids.sort_unstable();
    ids.dedup();

    for id in ids {
        let before = base.get(id);
        let after = theirs.get(id);
        let current = ours.get(id);

        if before == current {
            // Nobody else touched it, so the run's version is authoritative —
            // including its absence, which is a delete.
            match after {
                Some(value) => {
                    merged.insert(id.clone(), value.clone());
                }
                None => {
                    merged.remove(id);
                }
            }
        } else if before == after || after == current {
            // The run did not change it, or both sides changed it identically.
            // Either way the database's copy is already right.
        } else {
            conflicts.push(Conflict::new(domain, id.clone()));
        }
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocode_model::{Entity, Page, Project};

    fn project_with(names: &[&str]) -> Project {
        let mut project = Project::new("App");
        for name in names {
            let entity = Entity::new(*name);
            project.entities.insert(entity.id.clone(), entity);
        }
        project
    }

    fn entity_id(project: &Project, name: &str) -> String {
        project
            .entities
            .values()
            .find(|entity| entity.name == name)
            .map(|entity| entity.id.to_string())
            .expect("entity exists")
    }

    fn names(project: &Project) -> Vec<String> {
        let mut names: Vec<String> = project
            .entities
            .values()
            .map(|entity| entity.name.clone())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn disjoint_edits_are_both_kept() {
        let base = project_with(&["Expense"]);
        // The run adds an entity.
        let mut theirs = base.clone();
        let note = Entity::new("Note");
        theirs.entities.insert(note.id.clone(), note);
        // The database gained a different one meanwhile.
        let mut ours = base.clone();
        let approval = Entity::new("Approval");
        ours.entities.insert(approval.id.clone(), approval);

        let Rebase::Clean(merged) = rebase(&base, &theirs, &ours).unwrap() else {
            panic!("expected a clean merge");
        };
        assert_eq!(names(&merged), vec!["Approval", "Expense", "Note"]);
    }

    #[test]
    fn the_same_object_changed_on_both_sides_is_a_conflict() {
        let base = project_with(&["Expense"]);
        let id = entity_id(&base, "Expense");

        let mut theirs = base.clone();
        theirs.entities.values_mut().next().unwrap().name = "Cost".into();

        let mut ours = base.clone();
        ours.entities.values_mut().next().unwrap().name = "Spend".into();

        let Rebase::Conflicts(conflicts) = rebase(&base, &theirs, &ours).unwrap() else {
            panic!("expected a conflict");
        };
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].domain, "entities");
        assert_eq!(conflicts[0].object_id, id);
    }

    #[test]
    fn an_identical_change_on_both_sides_is_not_a_conflict() {
        let base = project_with(&["Expense"]);
        let mut theirs = base.clone();
        theirs.entities.values_mut().next().unwrap().name = "Cost".into();
        let ours = theirs.clone();

        assert!(matches!(
            rebase(&base, &theirs, &ours).unwrap(),
            Rebase::Clean(_)
        ));
    }

    #[test]
    fn a_delete_that_the_other_side_did_not_touch_is_applied() {
        let base = project_with(&["Expense", "Note"]);
        let mut theirs = base.clone();
        let note = entity_id(&theirs, "Note");
        let key = theirs
            .entities
            .iter()
            .find(|(_, entity)| entity.name == "Note")
            .map(|(id, _)| id.clone())
            .unwrap();
        theirs.entities.remove(&key);

        let Rebase::Clean(merged) = rebase(&base, &theirs, &base.clone()).unwrap() else {
            panic!("expected a clean merge");
        };
        assert_eq!(names(&merged), vec!["Expense"]);
        assert!(!merged.entities.contains_key(&key), "deleted {note}");
    }

    #[test]
    fn a_delete_that_races_an_edit_is_a_conflict() {
        let base = project_with(&["Expense", "Note"]);
        let mut theirs = base.clone();
        let key = theirs
            .entities
            .iter()
            .find(|(_, entity)| entity.name == "Note")
            .map(|(id, _)| id.clone())
            .unwrap();
        theirs.entities.remove(&key);

        let mut ours = base.clone();
        ours.entities.get_mut(&key).unwrap().description = Some("edited".into());

        assert!(matches!(
            rebase(&base, &theirs, &ours).unwrap(),
            Rebase::Conflicts(_)
        ));
    }

    #[test]
    fn a_new_object_that_only_the_run_created_is_inserted() {
        let base = project_with(&[]);
        let mut theirs = base.clone();
        let page = Page::new("Expenses");
        theirs.pages.insert(page.id.clone(), page);
        let ours = base.clone();

        let Rebase::Clean(merged) = rebase(&base, &theirs, &ours).unwrap() else {
            panic!("expected a clean merge");
        };
        assert_eq!(merged.pages.len(), 1);
    }

    #[test]
    fn the_active_theme_follows_the_side_that_moved_it() {
        let base = project_with(&[]);
        let mut theirs = base.clone();
        let theme = nocode_model::Theme::new("Dark");
        theirs.themes.insert(theme.id.clone(), theme.clone());
        theirs.active_theme_id = Some(theme.id.clone());

        let Rebase::Clean(merged) = rebase(&base, &theirs, &base.clone()).unwrap() else {
            panic!("expected a clean merge");
        };
        assert_eq!(merged.active_theme_id, Some(theme.id));
    }

    #[test]
    fn two_different_active_themes_conflict() {
        let base = project_with(&[]);
        let mut theirs = base.clone();
        let dark = nocode_model::Theme::new("Dark");
        theirs.themes.insert(dark.id.clone(), dark.clone());
        theirs.active_theme_id = Some(dark.id.clone());

        let mut ours = base.clone();
        let light = nocode_model::Theme::new("Light");
        ours.themes.insert(light.id.clone(), light.clone());
        ours.active_theme_id = Some(light.id.clone());

        let Rebase::Conflicts(conflicts) = rebase(&base, &theirs, &ours).unwrap() else {
            panic!("expected a conflict");
        };
        assert!(conflicts
            .iter()
            .any(|conflict| conflict.object_id == "active_theme"));
    }
}
