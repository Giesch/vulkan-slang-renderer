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
/// is self-contained. `shaders init` writes them into a consumer's source dir:
/// the `mltrs.slang` prelude at the top level, the modules it re-exports
/// under `mltrs/`.
const VENDORED_MODULES: &[(&str, &str)] = &[
    ("mltrs.slang", include_str!("../vendor/mltrs.slang")),
    (
        "mltrs/addr.slang",
        include_str!("../vendor/mltrs/addr.slang"),
    ),
    ("mltrs/mvp.slang", include_str!("../vendor/mltrs/mvp.slang")),
    (
        "mltrs/projection.slang",
        include_str!("../vendor/mltrs/projection.slang"),
    ),
    (
        "mltrs/fullscreen_triangle.slang",
        include_str!("../vendor/mltrs/fullscreen_triangle.slang"),
    ),
    (
        "mltrs/super_sample.slang",
        include_str!("../vendor/mltrs/super_sample.slang"),
    ),
];

/// Top-level engine module files from the pre-namespace layout; `shaders init`
/// removes these so they don't collide with the `mltrs/` copies in reflection.
const LEGACY_MODULES: &[&str] = &[
    "addr.slang",
    "mvp.slang",
    "projection.slang",
    "fullscreen_triangle.slang",
    "super_sample.slang",
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

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        println!("wrote {}", path.display());
    }

    for file_name in LEGACY_MODULES {
        let path = args.dir.join(file_name);
        if !path.exists() {
            continue;
        }
        if !args.force {
            anyhow::bail!(
                "found {} from the pre-namespace engine layout; its contents now \
                live in mltrs/{file_name}. re-run with --force to remove it",
                path.display()
            );
        }
        std::fs::remove_file(&path)?;
        println!("removed legacy {}", path.display());
    }

    Ok(())
}

fn compile(args: CompileArgs) -> anyhow::Result<()> {
    let crate_dir = &args.crate_dir;
    let config = build_tasks::Config {
        generate_rust_source: true,
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
