use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};

use mltrs_cli::build_tasks::{self, Config};

#[derive(Parser)]
#[command(name = "mltrs", version, about = "Slang shader tooling for mltrs")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Work with this crate's slang shaders
    #[command(subcommand)]
    Shaders(ShadersCommand),
}

#[derive(Subcommand)]
enum ShadersCommand {
    /// Compile slang shaders to spir-v + reflection json, and generate rust bindings
    Compile(CompileArgs),
    /// Copy the engine slang modules into a crate's shader source directory
    Init(InitArgs),
}

#[derive(Args)]
struct CompileArgs {
    /// The crate to compile shaders for; the other paths default relative to it
    #[arg(long, default_value = ".")]
    crate_dir: PathBuf,

    /// Where to read *.slang from [default: <crate-dir>/shaders/source]
    #[arg(long)]
    source_dir: Option<PathBuf>,

    /// Where to write spir-v and reflection json [default: <crate-dir>/shaders/compiled]
    #[arg(long)]
    compiled_dir: Option<PathBuf>,

    /// Where to write generated rust [default: <crate-dir>/src]
    #[arg(long)]
    rust_dir: Option<PathBuf>,

    /// The crate path generated code imports the engine from
    #[arg(long, default_value = "mltrs")]
    import_root: String,

    /// Only compile shaders; skip generating rust bindings
    #[arg(long)]
    no_rust: bool,
}

#[derive(Args)]
struct InitArgs {
    /// Where to write the engine slang modules
    #[arg(long, default_value = "shaders/source")]
    dir: PathBuf,

    /// Overwrite modules that already exist and differ
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

fn compile(args: CompileArgs) -> anyhow::Result<()> {
    validate_import_root(&args.import_root)?;

    let crate_dir = &args.crate_dir;
    let config = Config {
        generate_rust_source: !args.no_rust,
        rust_source_dir: args.rust_dir.unwrap_or_else(|| crate_dir.join("src")),
        shaders_source_dir: args
            .source_dir
            .unwrap_or_else(|| crate_dir.join("shaders").join("source")),
        compiled_shaders_dir: args
            .compiled_dir
            .unwrap_or_else(|| crate_dir.join("shaders").join("compiled")),
        import_root: args.import_root,
    };

    build_tasks::write_precompiled_shaders(config)
}

/// Rejects a malformed root early. Otherwise the bad path is baked into every
/// generated file and only surfaces as a wall of unresolved-import errors.
fn validate_import_root(import_root: &str) -> anyhow::Result<()> {
    let valid = !import_root.is_empty()
        && import_root.split("::").all(|segment| {
            !segment.is_empty()
                && !segment.starts_with(|c: char| c.is_ascii_digit())
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
        });

    if !valid {
        bail!("--import-root must be `crate` or a rust path like `mltrs`, got {import_root:?}");
    }

    Ok(())
}

fn init(args: InitArgs) -> anyhow::Result<()> {
    std::fs::create_dir_all(&args.dir)
        .with_context(|| format!("failed to create {}", args.dir.display()))?;

    let mut written = 0;
    let mut skipped = Vec::new();
    for (file_name, contents) in build_tasks::ENGINE_SLANG_MODULES {
        let path = args.dir.join(file_name);

        if !args.force && path.exists() {
            let existing = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            if existing == *contents {
                continue;
            }
            skipped.push(file_name);
            continue;
        }

        std::fs::write(&path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;
        written += 1;
    }

    println!(
        "wrote {written} engine slang module(s) to {}",
        args.dir.display()
    );
    if !skipped.is_empty() {
        println!("skipped {skipped:?} (locally modified; pass --force to overwrite)");
    }

    Ok(())
}
