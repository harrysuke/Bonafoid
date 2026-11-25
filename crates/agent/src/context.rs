//! Context cards: the anti-hallucination device.
//!
//! Between pipeline steps the server — never the model — builds a compact
//! document of the ids that actually exist. Step 3 therefore receives the
//! entity ids created in Step 1 instead of guessing UUIDs.
//!
//! The card is deliberately the same `catalog` document the MCP resource
//! `project://{id}/catalog` serves, so an agent and the pipeline see one shape.

use nocode_model::Project;
use serde_json::Value;

/// Cap on the rendered card. A card that does not fit is truncated with a note
/// rather than blowing the model's context window: the agent can still call
/// `project_get` for the part it needs.
const MAX_CARD_CHARS: usize = 8_000;

/// The structured card.
pub fn card(project: &Project) -> Value {
    nocode_mcp::resources::project::catalog(project)
}

/// The card as pretty JSON, truncated if necessary.
pub fn render(project: &Project) -> String {
    let text = serde_json::to_string_pretty(&card(project)).unwrap_or_else(|_| "{}".to_string());
    if text.len() <= MAX_CARD_CHARS {
        return text;
    }
    let mut truncated: String = text.chars().take(MAX_CARD_CHARS).collect();
    truncated.push_str(
        "\n… truncated. Call project_get with domain=entities|forms|pages|workflows for the rest.",
    );
    truncated
}

/// A one-line summary for the `run_started` event and the audit row.
pub fn headline(project: &Project) -> String {
    format!(
        "{}: {} entities, {} workflows, {} forms, {} pages",
        project.name,
        project.entities.len(),
        project.workflows.len(),
        project.forms.len(),
        project.pages.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocode_model::Entity;

    #[test]
    fn the_card_lists_real_entity_ids() {
        let mut project = Project::new("App");
        let entity = Entity::new("Expense");
        let id = entity.id.to_string();
        project.entities.insert(entity.id.clone(), entity);

        let card = card(&project);
        assert_eq!(card["entities"][0]["id"], id);
    }

    #[test]
    fn rendering_is_bounded() {
        let mut project = Project::new("Big");
        for index in 0..400 {
            let entity = Entity::new(format!("Entity{index}"));
            project.entities.insert(entity.id.clone(), entity);
        }
        let rendered = render(&project);
        assert!(rendered.len() <= MAX_CARD_CHARS + 200);
        assert!(rendered.contains("truncated"));
    }
}
