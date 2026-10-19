use tauri::{Manager, State};

use nocode_ide::{
    export_bpmn_xml, import_bpmn, run_workflow, validate_workflow, GenerationReport, IdeService,
    RunOptions, RunSummary, WorkflowValidation,
};

#[tauri::command]
async fn new_project(name: String, state: State<'_, IdeService>) -> Result<String, String> {
    state.new_project(&name)
}

#[tauri::command]
async fn seed_sample_project(state: State<'_, IdeService>) -> Result<String, String> {
    state.seed_sample_project()
}

#[tauri::command]
async fn set_db_driver(driver: String, state: State<'_, IdeService>) -> Result<String, String> {
    state.set_db_driver(&driver)
}

#[tauri::command]
async fn set_frontend_framework(
    framework: String,
    state: State<'_, IdeService>,
) -> Result<String, String> {
    state.set_frontend_framework(&framework)
}

#[tauri::command]
async fn save_project(path: String, state: State<'_, IdeService>) -> Result<(), String> {
    state.save_project(&path)
}

#[tauri::command]
async fn load_project(path: String, state: State<'_, IdeService>) -> Result<String, String> {
    state.load_project(&path)
}

#[tauri::command]
async fn get_current_project(state: State<'_, IdeService>) -> Result<Option<String>, String> {
    state.get_current_project()
}

#[tauri::command]
async fn get_project_path(state: State<'_, IdeService>) -> Result<Option<String>, String> {
    Ok(state.get_project_path())
}

#[tauri::command]
async fn update_project(project_json: String, state: State<'_, IdeService>) -> Result<(), String> {
    state.update_project(&project_json)
}

#[tauri::command]
async fn generate_code(
    state: State<'_, IdeService>,
    output_dir: String,
) -> Result<GenerationReport, String> {
    state.generate_code(&output_dir).await
}

#[tauri::command]
async fn bpmn_import(xml: String, name: Option<String>) -> Result<String, String> {
    let name = name.unwrap_or_else(|| "Imported Process".to_string());
    import_bpmn(&xml, &name)
}

#[tauri::command]
async fn bpmn_export(workflow_json: String) -> Result<String, String> {
    export_bpmn_xml(&workflow_json)
}

#[tauri::command]
async fn bpmn_validate(workflow_json: String) -> Result<WorkflowValidation, String> {
    validate_workflow(&workflow_json)
}

#[tauri::command]
async fn workflow_run(
    workflow_json: String,
    variables: Option<serde_json::Value>,
    auto_complete_tasks: Option<bool>,
) -> Result<RunSummary, String> {
    run_workflow(
        &workflow_json,
        variables.unwrap_or(serde_json::Value::Null),
        RunOptions {
            auto_complete_tasks: auto_complete_tasks.unwrap_or(false),
            max_rounds: None,
        },
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .manage(IdeService::new())
        .invoke_handler(tauri::generate_handler![
            new_project,
            seed_sample_project,
            set_db_driver,
            set_frontend_framework,
            save_project,
            load_project,
            get_current_project,
            get_project_path,
            update_project,
            generate_code,
            bpmn_import,
            bpmn_export,
            bpmn_validate,
            workflow_run,
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
