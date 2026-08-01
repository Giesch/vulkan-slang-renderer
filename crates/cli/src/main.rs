use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use mltrs_cli::build_tasks;

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
    /// Compile slang shaders to SPIR-V + reflection json, and generate Rust bindings
    Compile(CompileArgs),
    /// Seed a shaders/source dir with the vendored engine slang modules
    Init(InitArgs),
}

#[derive(Args)]
struct CompileArgs {
    /// the consuming crate's root directory
    #[arg(long, default_value = ".")]
    crate_dir: PathBuf,
    /// slang source dir (default: <crate-dir>/shaders/source)
    #[arg(long)]
    source_dir: Option<PathBuf>,
    /// compiled spirv/json dir (default: <crate-dir>/shaders/compiled)
    #[arg(long)]
    compiled_dir: Option<PathBuf>,
    /// rust source dir to write the generated module into (default: <crate-dir>/src)
    #[arg(long)]
    rust_dir: Option<PathBuf>,
    /// path prefix generated imports use for the engine crate
    #[arg(long, default_value = "mltrs")]
    import_root: String,
    /// only write spirv + json, skip rust codegen
    #[arg(long)]
    no_rust: bool,
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

/// The canonical engine slang modules, embedded so `cargo install mltrs-cli`
/// is self-contained. `shaders init` writes them into a consumer's source dir.
const VENDORED_MODULES: &[(&str, &str)] = &[
    ("addr.slang", include_str!("../vendor/addr.slang")),
    ("mvp.slang", include_str!("../vendor/mvp.slang")),
    (
        "projection.slang",
        include_str!("../vendor/projection.slang"),
    ),
    (
        "fullscreen_triangle.slang",
        include_str!("../vendor/fullscreen_triangle.slang"),
    ),
    (
        "super_sample.slang",
        include_str!("../vendor/super_sample.slang"),
    ),
];

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

        std::fs::write(&path, content)?;
        println!("wrote {}", path.display());
    }

    Ok(())
}

fn compile(args: CompileArgs) -> anyhow::Result<()> {
    let crate_dir = &args.crate_dir;
    let config = build_tasks::Config {
        generate_rust_source: !args.no_rust,
        rust_source_dir: args.rust_dir.unwrap_or_else(|| crate_dir.join("src")),
        shaders_source_dir: args
            .source_dir
            .unwrap_or_else(|| crate_dir.join("shaders/source")),
        compiled_shaders_dir: args
            .compiled_dir
            .unwrap_or_else(|| crate_dir.join("shaders/compiled")),
        import_root: args.import_root,
    };

    build_tasks::write_precompiled_shaders(config)
}
