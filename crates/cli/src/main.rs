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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Shaders(ShadersCommand::Compile(args)) => compile(args),
    }
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
