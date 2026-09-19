use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use mltrs_cli::build_tasks;
use mltrs_cli::build_tasks::VENDORED_MODULES;

#[derive(Parser)]
#[command(name = "mltrs", version, about = "mltrs engine tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Shader compilation and project setup
    #[command(subcommand)]
    Shaders(ShadersCommand),
}

#[derive(Subcommand)]
enum ShadersCommand {
    /// Compile Slang shaders and generate Rust or Roc bindings
    Compile(CompileArgs),
    /// Seed a shaders/source dir with the vendored engine slang module
    Init(InitArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Language {
    Rust,
    Roc,
}

#[derive(Args)]
struct CompileArgs {
    /// the consuming project's root directory
    #[arg(long, default_value = ".")]
    crate_dir: PathBuf,
    /// output language; otherwise inferred from main.roc or Cargo.toml in crate-dir
    #[arg(long, value_enum)]
    language: Option<Language>,
    /// Slang source dir (default: <project-dir>/shaders/source)
    #[arg(long)]
    source_dir: Option<PathBuf>,
    /// compiled SPIR-V/JSON dir (default: <crate-dir>/shaders/compiled)
    #[arg(long)]
    compiled_dir: Option<PathBuf>,
    /// Rust source dir to write generated modules into (default: <crate-dir>/src)
    #[arg(long)]
    rust_dir: Option<PathBuf>,
    /// Roc source dir managed by codegen (default: <crate-dir>/Generated)
    #[arg(long)]
    roc_dir: Option<PathBuf>,
    /// path prefix generated Rust imports use for the engine crate
    #[arg(long)]
    import_root: Option<String>,
}

#[derive(Args)]
struct InitArgs {
    /// directory to write the engine slang modules into
    #[arg(long, default_value = "shaders/source")]
    dir: PathBuf,
    /// overwrite existing (possibly modified) files
    #[arg(long)]
    force: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Shaders(ShadersCommand::Compile(args)) => compile(args),
        Command::Shaders(ShadersCommand::Init(args)) => init(args),
    }
}

fn init(args: InitArgs) -> anyhow::Result<()> {
    std::fs::create_dir_all(&args.dir)?;

    for (file_name, content) in VENDORED_MODULES {
        let path = args.dir.join(file_name);

        if path.exists() && !args.force {
            let existing = std::fs::read_to_string(&path)?;
            if existing == *content {
                continue;
            }

            anyhow::bail!(
                "refusing to overwrite modified {}; re-run with --force",
                path.display()
            );
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&path, content)?;
        println!("wrote {}", path.display());
    }

    Ok(())
}

fn compile(args: CompileArgs) -> anyhow::Result<()> {
    let crate_dir = &args.crate_dir;
    let language = match args.language {
        Some(language) => language,
        None => {
            let roc_marker = crate_dir.join("main.roc").is_file();
            let rust_marker = crate_dir.join("Cargo.toml").is_file();
            match (roc_marker, rust_marker) {
                (true, false) => Language::Roc,
                (false, true) => Language::Rust,
                (true, true) => anyhow::bail!(
                    "both main.roc and Cargo.toml exist directly in {}; select one with \
                     --language rust|roc",
                    crate_dir.display()
                ),
                (false, false) => anyhow::bail!(
                    "neither main.roc nor Cargo.toml exists directly in {}; select one with \
                     --language rust|roc",
                    crate_dir.display()
                ),
            }
        }
    };

    let shaders_source_dir = args
        .source_dir
        .unwrap_or_else(|| crate_dir.join("shaders/source"));
    let compiled_shaders_dir = args
        .compiled_dir
        .unwrap_or_else(|| crate_dir.join("shaders/compiled"));

    match language {
        Language::Rust => {
            if args.roc_dir.is_some() {
                anyhow::bail!("--roc-dir is only valid with --language roc");
            }

            let config = build_tasks::Config {
                generate_rust_source: true,
                rust_source_dir: args.rust_dir.unwrap_or_else(|| crate_dir.join("src")),
                shaders_source_dir,
                compiled_shaders_dir,
                import_root: args.import_root.unwrap_or_else(|| "mltrs".to_string()),
                optimization: build_tasks::OptimizationLevel::High,
            };

            build_tasks::write_precompiled_shaders(config)
        }

        Language::Roc => {
            if args.rust_dir.is_some() {
                anyhow::bail!("--rust-dir is only valid with --language rust");
            }

            if args.import_root.is_some() {
                anyhow::bail!("--import-root is only valid with --language rust");
            }

            let config = mltrs_cli::roc_codegen::RocConfig {
                project_root: crate_dir.clone(),
                roc_source_dir: args.roc_dir.unwrap_or_else(|| crate_dir.join("Generated")),
                shaders_source_dir,
                compiled_shaders_dir,
                optimization: build_tasks::OptimizationLevel::High,
            };

            mltrs_cli::roc_codegen::write_precompiled_roc_shaders(config)
        }
    }
}
