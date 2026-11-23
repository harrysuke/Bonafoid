# Bonafoid workflow authoring guide

Use the granular graph tools. Do **not** generate BPMN XML yourself: the
`export_bpmn_xml` tool is an interop endpoint, not an authoring surface.

## Preferred sequence

1. Read `bpmn://catalog/node-types` for valid `node_type` tags and config keys.
2. `create_workflow(name=...)` — keep the returned `session_id`.
3. Add nodes one at a time with `add_node`. Omit `position`; the server assigns
   IDs and (by default) coordinates. Put gateway conditions on **outgoing
   edges**, never on the gateway node.
4. Connect nodes with `connect_nodes`. Both endpoints must already exist.
5. `auto_layout` once after a batch of edits. It is the only owner of
   `NodePosition`; manually supplied positions are hints that layout will
   overwrite.
6. `validate_workflow`; fix every reported error before continuing.
7. `simulate_workflow` with representative variables. Use
   `auto_complete_tasks=true` to run human tasks to completion.
8. `save_workflow(target="project", project_id=...)` so the human can open the
   result in the visual Workflow Builder. Report validation results to them.

## Rules

- Valid `node_type` values are the 16 tags in `bpmn://catalog/node-types`.
  Unknown tags and unknown config keys are rejected.
- Every non-end node needs an outgoing flow; every non-start node needs an
  incoming flow. One start event is required.
- Conditions are short expressions such as `approved == true`, `amount > 100`
  or `!( rejected == true )`. See the IDE expression evaluator semantics in
  `crates/bpmn/src/expr.rs`.
- Mutations accept `expected_revision` for optimistic concurrency. On a stale
  revision, call `get_workflow` and retry with the current revision.
- `undo`/`redo` keep a bounded snapshot history (default depth 20).
- `get_workflow(view="summary")` is the cheap way to inspect a graph; the full
  view includes all node configs and positions.
