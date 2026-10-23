//! Regression tests for the generated application's deployability.
//!
//! These exist because a generated app that does not COMPILE, or whose
//! container image cannot build, is not a working app. The assertions here
//! are the ones a user hits first: the frontend must reference only
//! functions that exist, the Docker/Kubernetes output must not depend on
//! files the generator does not produce, and the deployment files must
//! match the project's database driver.

use crate::project::{generate_project_in_memory, GeneratedProject};
use nocode_model::entity::{Entity, EntityField, EntityFieldId, EntityFieldType};
use nocode_model::page::{
    ActionConfig, ActionId, ActionType, ComponentId, ComponentType, DataSource, DataSourceType,
    DatabaseQuery, OrderByClause, OrderDirection, Page, PageAction, PaginationConfig,
    SelectionConfig, TableColumn,
};
use nocode_model::{DatabaseDriver, Project};

/// The relative paths of every artifact the generator produced.
fn generated_paths(generated: &GeneratedProject) -> Vec<String> {
    generated
        .files()
        .iter()
        .map(|(path, _)| path.to_string_lossy().replace('\\', "/"))
        .collect()
}

/// A project with one entity, one form, and a list page whose data source
/// is `Database`-typed and whose action is a `CallApi` — exactly what the
/// IDE's List Builder produces.
pub(crate) fn project_with_a_list_page() -> (Project, String) {
    let mut project = Project::new("Acme");

    let mut entity = Entity::new("Product");
    entity.fields.push(EntityField {
        id: EntityFieldId::new(),
        name: "id".to_string(),
        field_type: EntityFieldType::Uuid,
        nullable: false,
        unique: true,
        default: None,
        auto_generate: None,
        validation: None,
        comment: None,
        order: 0,
    });
    let entity_id = entity.id.clone();
    project.entities.insert(entity_id.clone(), entity);

    // The List Builder's data source: a UUID id, a display name, and a
    // Database source type.
    let data_source_id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".to_string();
    let query = DatabaseQuery {
        select: vec![],
        r#where: vec![],
        order_by: vec![OrderByClause {
            field: "id".to_string(),
            direction: OrderDirection::Asc,
        }],
        limit: None,
        offset: None,
        group_by: vec![],
        having: vec![],
    };
    let data_source = DataSource {
        id: data_source_id.clone(),
        name: "Product Data".to_string(),
        auto_load: true,
        polling_interval: None,
        transform: None,
        source_type: DataSourceType::Database {
            entity_id: entity_id.clone(),
            query,
        },
        config: nocode_model::page::DataSourceConfig {
            timeout_ms: 30_000,
            retry: Default::default(),
            cache: Default::default(),
            debounce_ms: None,
            transform_response: None,
            transform_error: None,
        },
    };

    let mut page = Page::new("Products List");
    page.path = "/products/list".to_string();
    page.components.push(nocode_model::page::PageComponent {
        id: ComponentId::new(),
        component: ComponentType::Table {
            columns: vec![TableColumn {
                key: "id".to_string(),
                title: "Id".to_string(),
                data_index: "id".to_string(),
                width: None,
                min_width: None,
                max_width: None,
                fixed: None,
                align: None,
                ellipsis: false,
                sortable: true,
                filterable: false,
                render: None,
                children: vec![],
            }],
            data_source: data_source_id.clone(),
            pagination: PaginationConfig::default(),
            sorting: true,
            filtering: false,
            selection: SelectionConfig::default(),
        },
        props: Default::default(),
        bindings: vec![],
        events: vec![],
        conditions: vec![],
        slots: Default::default(),
        style_overrides: Default::default(),
    });
    page.actions.push(PageAction {
        id: ActionId::new(),
        name: "Create Record".to_string(),
        action_type: ActionType::CallApi,
        config: ActionConfig::CallApi {
            data_source_id: data_source_id.clone(),
            params: Default::default(),
            on_success: vec![],
            on_error: vec![],
        },
        permissions: vec![],
    });
    page.data_sources.push(data_source);
    project.pages.insert(page.id.clone(), page);

    (project, data_source_id)
}

/// Every `api::` function the generated frontend calls.
fn called_api_functions(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = source;
    while let Some(index) = rest.find("api::") {
        rest = &rest[index + "api::".len()..];
        let end = rest
            .find(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .unwrap_or(rest.len());
        if end > 0 {
            names.push(rest[..end].to_string());
        }
    }
    names
}

/// Every `api::` function the generated frontend defines.
fn defined_api_functions(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("pub async fn ") {
            if let Some(name) = rest.split('(').next() {
                names.push(name.to_string());
            }
        }
    }
    names
}

#[tokio::test]
async fn a_list_page_action_only_calls_an_api_function_that_exists() {
    let (project, data_source_id) = project_with_a_list_page();
    let generated = generate_project_in_memory(&project)
        .await
        .expect("generate the project in memory");

    let api = generated
        .file("frontend/src/api.rs")
        .expect("the generated api client");
    // The USER page (as opposed to the auto-generated entity page) is the
    // one the List Builder produces. It has a distinctive header comment,
    // so the test does not depend on the naming rule.
    let page_path = generated_paths(&generated)
        .into_iter()
        .filter(|path| path.starts_with("frontend/src/pages/"))
        .find(|path| {
            generated
                .file(path)
                .map(|content| content.contains("Rendered page component"))
                .unwrap_or(false)
        })
        .expect("the generated user page");
    let page = generated
        .file(&page_path)
        .expect("the generated user page content");

    let defined = defined_api_functions(&api);
    for called in called_api_functions(&page) {
        assert!(
            defined.contains(&called),
            "the generated page calls api::{called}, which the generated api client does not \
             define (it defines: {defined:?})"
        );
    }

    // The data source id is a UUID, so it can never be a Rust identifier.
    assert!(
        !page.contains(&data_source_id),
        "the generated page embeds a raw data source id as a function name"
    );
}

#[tokio::test]
async fn the_generated_dockerfile_does_not_copy_a_file_that_is_not_generated() {
    let (project, _) = project_with_a_list_page();
    let generated = generate_project_in_memory(&project)
        .await
        .expect("generate the project in memory");

    let dockerfile = generated.file("docker/Dockerfile").expect("the Dockerfile");
    let generated_files = generated_paths(&generated);

    // Every `COPY <src> <dst>` source that is a project-relative path must
    // correspond to a file the generator actually produced.
    for line in dockerfile.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("COPY ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(sources) = parts.next() else {
            continue;
        };
        for source in sources.split_whitespace() {
            if source.starts_with("--") || source.contains('$') {
                continue;
            }
            let relative = source.trim_end_matches('/');
            // Only project-relative paths are checked; the builder stage also
            // copies from its own image layers. A path may be a FILE or a
            // DIRECTORY (the Dockerfile copies `backend/src` wholesale), so
            // it resolves if it is a generated file or a prefix of one.
            if relative.starts_with("backend/") || relative.starts_with("frontend/") {
                let resolves = generated_files
                    .iter()
                    .any(|file| file == relative || file.starts_with(&format!("{relative}/")));
                assert!(
                    resolves,
                    "the Dockerfile copies {relative}, which the generator does not produce"
                );
            }
        }
    }
}

#[tokio::test]
async fn the_generated_docker_health_check_uses_a_tool_the_image_contains() {
    let (project, _) = project_with_a_list_page();
    let generated = generate_project_in_memory(&project)
        .await
        .expect("generate the project in memory");

    let dockerfile = generated.file("docker/Dockerfile").expect("the Dockerfile");
    let healthcheck = dockerfile
        .lines()
        .find(|line| line.trim_start().starts_with("HEALTHCHECK"))
        .expect("the Dockerfile has a health check");

    // The runtime stage must install whatever the health check shells out to.
    let runtime_stage = dockerfile
        .split("FROM debian")
        .nth(1)
        .expect("a debian runtime stage");
    if healthcheck.contains("curl") {
        assert!(
            runtime_stage.contains("curl"),
            "the health check shells out to curl, but the runtime image does not install it"
        );
    }
}

#[tokio::test]
async fn the_generated_compose_file_mounts_paths_that_exist() {
    let (project, _) = project_with_a_list_page();
    let generated = generate_project_in_memory(&project)
        .await
        .expect("generate the project in memory");

    let compose = generated
        .file("docker/docker-compose.yml")
        .expect("the compose file");
    let generated_files = generated_paths(&generated);

    for line in compose.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("- ") {
            continue;
        }
        let Some((_service, path)) = trimmed.split_once(':') else {
            continue;
        };
        let path = path.trim();
        if !path.starts_with('.') {
            continue;
        }
        // Compose resolves relative paths against the compose file's own
        // directory, which is `docker/`. A path is valid if it resolves to
        // something the generator produced, either directly or as a
        // directory prefix.
        let relative = path.trim_start_matches("./");
        let candidate = format!("docker/{relative}");
        let parent = relative
            .rsplit_once('/')
            .map(|(head, _)| head.to_string())
            .unwrap_or_default();
        let resolves = generated_files.iter().any(|file| {
            file == &candidate
                || file.starts_with(&format!("{candidate}/"))
                || file.starts_with(&format!("docker/{parent}/"))
        });
        assert!(
            resolves,
            "the compose file mounts {path}, which does not resolve to a generated path"
        );
    }
}

#[tokio::test]
async fn the_generated_readme_does_not_reference_a_file_that_is_not_generated() {
    let (project, _) = project_with_a_list_page();
    let generated = generate_project_in_memory(&project)
        .await
        .expect("generate the project in memory");

    let readme = generated.file("README.md").expect("the README");
    let generated_files = generated_paths(&generated);

    // A dot-file named in prose must exist. The README used to tell the
    // user to copy `.env.example`, which the generator never produced.
    let mention = ".env.example";
    if readme.contains(mention) {
        assert!(
            generated_files.iter().any(|file| file == mention),
            "the README tells the user to use {mention}, which is not generated"
        );
    }
}

/// The compose file must match the project's driver.
///
/// A SQLite project needs no database server, so starting a Postgres for it
/// would be wrong — and it would be a needless thing for the user to
/// install. This used to be one hardcoded Postgres service.
#[tokio::test]
async fn the_compose_file_matches_the_project_driver() {
    for (driver, service, url_fragment) in [
        (DatabaseDriver::SQLite, "", "sqlite://"),
        (DatabaseDriver::Postgres, "postgres:", "postgres://"),
        (DatabaseDriver::MySQL, "mysql:", "mysql://"),
    ] {
        let (mut project, _) = project_with_a_list_page();
        project.settings.database.driver = driver.clone();
        let generated = generate_project_in_memory(&project)
            .await
            .expect("generate the project in memory");

        let compose = generated
            .file("docker/docker-compose.yml")
            .expect("the compose file");

        assert!(
            compose.contains(&format!("DATABASE_URL: {url_fragment}")),
            "the compose DATABASE_URL does not match the {driver:?} driver:\n{compose}"
        );
        assert!(
            compose.contains(service),
            "the compose file does not mention the {driver:?} service:\n{compose}"
        );
    }
}

/// A SQLite project must not start a database server.
#[tokio::test]
async fn a_sqlite_project_starts_no_database_server() {
    let (mut project, _) = project_with_a_list_page();
    project.settings.database.driver = DatabaseDriver::SQLite;
    let generated = generate_project_in_memory(&project)
        .await
        .expect("generate the project in memory");

    let compose = generated
        .file("docker/docker-compose.yml")
        .expect("the compose file");

    assert!(
        !compose.contains("postgres:16-alpine") && !compose.contains("mysql:8"),
        "a SQLite project must not start a database server:\n{compose}"
    );
    // The database is a file, so it needs somewhere to live.
    assert!(
        compose.contains("app_data:/app/data"),
        "the SQLite data file has no volume:\n{compose}"
    );
}

/// The Kubernetes secret must carry the project's own driver.
#[tokio::test]
async fn the_kubernetes_secret_matches_the_project_driver() {
    for (driver, fragment) in [
        (DatabaseDriver::SQLite, "sqlite://"),
        (DatabaseDriver::Postgres, "postgres://"),
        (DatabaseDriver::MySQL, "mysql://"),
    ] {
        let (mut project, _) = project_with_a_list_page();
        project.settings.database.driver = driver.clone();
        let generated = generate_project_in_memory(&project)
            .await
            .expect("generate the project in memory");

        let secret = generated
            .file("kubernetes/secret.yaml")
            .expect("the secret");
        assert!(
            secret.contains(&format!("database-url: \"{fragment}")),
            "the k8s secret does not match the {driver:?} driver:\n{secret}"
        );
    }
}

/// A driver SeaORM cannot serve must be REFUSED, not guessed.
///
/// SeaORM 0.12 ships only the SQLite, MySQL and Postgres SQLx drivers. The
/// generator used to fall back to Postgres for everything else, so a
/// "SQL Server" project produced a Postgres backend under the wrong name.
#[tokio::test]
async fn a_driver_seaorm_cannot_serve_is_refused() {
    for driver in [DatabaseDriver::SqlServer, DatabaseDriver::Oracle] {
        let (mut project, _) = project_with_a_list_page();
        project.settings.database.driver = driver.clone();

        let error = generate_project_in_memory(&project)
            .await
            .expect_err("the generation must fail rather than guess a driver");
        let message = error.to_string();
        assert!(
            message.contains("SQL Server") || message.contains("Oracle"),
            "the refusal does not name the driver: {message}"
        );
    }
}

/// A new project must generate a working deployment with no database server.
///
/// This is the payoff of the SQLite default: the project that `Project::new`
/// produces has to be generatable and runnable as it stands.
#[tokio::test]
async fn a_new_project_generates_a_sqlite_deployment() {
    let project = Project::new("Acme");
    assert_eq!(project.settings.database.driver, DatabaseDriver::SQLite);

    let generated = generate_project_in_memory(&project)
        .await
        .expect("a new project must generate");

    let compose = generated
        .file("docker/docker-compose.yml")
        .expect("the compose file");
    assert!(
        compose.contains("DATABASE_URL: sqlite://"),
        "a new project's compose file is not SQLite:\n{compose}"
    );

    let cargo_toml = generated
        .file("backend/Cargo.toml")
        .expect("the backend manifest");
    assert!(
        cargo_toml.contains("sqlx-sqlite"),
        "a new project's backend is not built for SQLite:\n{cargo_toml}"
    );
    assert!(
        !cargo_toml.contains("sqlx-postgres"),
        "a new project's backend still asks for the Postgres driver:\n{cargo_toml}"
    );
}
