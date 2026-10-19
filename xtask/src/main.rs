use clap::{Parser, Subcommand};
use anyhow::Result;
use std::path::PathBuf;
use nocode_model::Project;
use nocode_codegen::{generate_project, generate_backend, generate_frontend, generate_database};

#[derive(Parser)]
#[command(name = "nocode-xtask")]
#[command(about = "No-Code IDE build tasks", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a complete project from a model file
    Generate {
        /// Path to the project model JSON file
        #[arg(short, long)]
        model: PathBuf,
        
        /// Output directory
        #[arg(short, long, default_value = "./generated")]
        output: PathBuf,
    },
    
    /// Generate only the backend
    Backend {
        #[arg(short, long)]
        model: PathBuf,
        
        #[arg(short, long, default_value = "./generated/backend")]
        output: PathBuf,
    },
    
    /// Generate only the frontend
    Frontend {
        #[arg(short, long)]
        model: PathBuf,
        
        #[arg(short, long, default_value = "./generated/frontend")]
        output: PathBuf,
    },
    
    /// Generate only database migrations
    Database {
        #[arg(short, long)]
        model: PathBuf,
        
        #[arg(short, long, default_value = "./generated/database")]
        output: PathBuf,
    },
    
    /// Build the Tauri app
    BuildTauri {
        #[arg(short, long, default_value = "release")]
        profile: String,
    },
    
    /// Build the frontend for web deployment
    BuildWeb {
        #[arg(short, long, default_value = "./generated/frontend")]
        frontend_dir: PathBuf,
    },

    /// Build the web frontend and its PWA assets of a generated project
    BuildPwa {
        #[arg(short, long, default_value = "./generated/frontend")]
        frontend_dir: PathBuf,
    },

    /// Build the Tauri desktop shell of a generated project
    BuildDesktop {
        #[arg(short, long, default_value = "./generated/desktop/src-tauri")]
        desktop_dir: PathBuf,
    },
    
    /// Create a new project scaffold
    New {
        /// Project name
        name: String,
        
        /// Output directory
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
    },

    /// Generate the model JSON Schema (source of truth for the TS types)
    Types {
        /// Output path (defaults to packages/ide-core/src/types/model.schema.json)
        #[arg(long)]
        schema: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Generate { model, output } => {
            let project = load_project(&model)?;
            println!("Generating project: {}", project.name);
            let generated = generate_project(&project, &output).await?;
            println!("Generated project at: {}", output.display());
            print_generated_summary(&generated);
        }
        
        Commands::Backend { model, output } => {
            let project = load_project(&model)?;
            println!("Generating backend for: {}", project.name);
            let backend = generate_backend(&project, &output).await?;
            println!("Generated backend at: {}", output.display());
        }
        
        Commands::Frontend { model, output } => {
            let project = load_project(&model)?;
            println!("Generating frontend for: {}", project.name);
            let frontend = generate_frontend(&project, &output).await?;
            println!("Generated frontend at: {}", output.display());
        }
        
        Commands::Database { model, output } => {
            let project = load_project(&model)?;
            println!("Generating database for: {}", project.name);
            let database = generate_database(&project, &output).await?;
            println!("Generated database at: {}", output.display());
            println!("Migrations: {}", database.migrations.len());
        }
        
        Commands::BuildTauri { profile } => {
            println!("Building Tauri app ({})...", profile);
            build_tauri(&profile).await?;
        }
        
        Commands::BuildWeb { frontend_dir } => {
            println!("Building web frontend...");
            build_web(&frontend_dir).await?;
        }

        Commands::BuildPwa { frontend_dir } => {
            println!("Building web frontend with PWA assets...");
            build_web(&frontend_dir).await?;
            let manifest = frontend_dir.join("public").join("manifest.webmanifest");
            if manifest.exists() {
                println!("PWA manifest present at: {}", manifest.display());
            } else {
                println!("Warning: no PWA manifest found at {}", manifest.display());
            }
        }

        Commands::BuildDesktop { desktop_dir } => {
            println!("Building Tauri desktop shell...");
            build_desktop(&desktop_dir).await?;
        }
        
        Commands::New { name, output } => {
            println!("Creating new project: {}", name);
            create_new_project(&name, &output)?;
        }

        Commands::Types { schema } => {
            let path = schema.unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .map(|root| root.join("packages/ide-core/src/types/model.schema.json"))
                    .unwrap_or_else(|| PathBuf::from("model.schema.json"))
            });
            let root = schemars::schema_for!(nocode_model::Project);
            let json = serde_json::to_string_pretty(&root)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, format!("{json}\n"))?;
            println!("Wrote JSON Schema to {}", path.display());
        }
    }
    
    Ok(())
}

fn load_project(path: &PathBuf) -> Result<Project> {
    let content = std::fs::read_to_string(path)?;
    let project: Project = serde_json::from_str(&content)?;
    Ok(project)
}

fn print_generated_summary(generated: &nocode_codegen::project::GeneratedProject) {
    println!("\n=== Generated Project Summary ===");
    println!("Project ID: {}", generated.project_id);
    println!("Output dir: {}", generated.output_dir.display());
    println!("\nBackend files: {}", generated.backend.files.len());
    println!("Frontend files: {}", generated.frontend.files.len());
    println!("Database migrations: {}", generated.database.migrations.len());
    println!("Docker files: 3");
    println!("Kubernetes files: 5");
    println!("PWA files: {}", generated.targets.pwa.files().len());
    println!("Desktop files: {}", generated.targets.desktop.files().len());
}

async fn build_desktop(desktop_dir: &PathBuf) -> Result<()> {
    let mut cmd = tokio::process::Command::new("cargo");
    cmd.arg("tauri").arg("build");
    cmd.current_dir(desktop_dir);
    let status = cmd.status().await?;
    if !status.success() {
        anyhow::bail!("Desktop build failed");
    }
    println!("Desktop build completed successfully");
    Ok(())
}

async fn build_tauri(profile: &str) -> Result<()> {
    let mut cmd = tokio::process::Command::new("cargo");
    cmd.arg("tauri").arg("build");
    if profile == "release" {
        cmd.arg("--release");
    }
    cmd.current_dir("./apps/ide-tauri");
    let status = cmd.status().await?;
    if !status.success() {
        anyhow::bail!("Tauri build failed");
    }
    println!("Tauri build completed successfully");
    Ok(())
}

async fn build_web(frontend_dir: &PathBuf) -> Result<()> {
    let mut cmd = tokio::process::Command::new("npm");
    cmd.arg("run").arg("build");
    cmd.current_dir(frontend_dir);
    let status = cmd.status().await?;
    if !status.success() {
        anyhow::bail!("Web build failed");
    }
    println!("Web build completed successfully");
    Ok(())
}

fn create_new_project(name: &str, output: &PathBuf) -> Result<()> {
    let project_dir = output.join(name.to_lowercase().replace(' ', "-"));
    std::fs::create_dir_all(&project_dir)?;
    
    // Create project model
    let project = Project::new(name);
    let model_json = serde_json::to_string_pretty(&project)?;
    std::fs::write(project_dir.join("project.json"), model_json)?;
    
    // Create basic structure
    std::fs::create_dir_all(project_dir.join("src"))?;
    std::fs::create_dir_all(project_dir.join("assets"))?;
    
    // Create README
    let readme = format!("# {}\n\nNo-Code IDE project generated by nocode-xtask.\n", name);
    std::fs::write(project_dir.join("README.md"), readme)?;
    
    println!("Created new project at: {}", project_dir.display());
    Ok(())
}