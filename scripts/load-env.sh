# Load environment variables from .env — the unix mirror of load-env.ps1.
#
# `.envrc` does this automatically, but only in an interactive shell with direnv
# hooked. Source this instead from a non-interactive one (CI, `bash -c`, an
# agent session):
#
#     . ./scripts/load-env.sh
#
# Note that `.cargo/config.toml` already sets the SLANG_* paths for anything
# cargo runs, so this is only needed when a *non-cargo* process needs them —
# running `target/debug/examples/<name>` directly, for instance, as
# scripts/headless-sweep.sh does.
#
# Parsing rather than sourcing (`set -a; . .env`) so that the `$PWD` in .env
# expands to the repo root regardless of the caller's working directory, which
# is exactly what load-env.ps1 does.

__load_env_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"

while IFS= read -r __load_env_line || [ -n "$__load_env_line" ]; do
    case "$__load_env_line" in
        '' | '#'*) continue ;;
    esac

    __load_env_name="${__load_env_line%%=*}"
    __load_env_value="${__load_env_line#*=}"
    # strip surrounding double quotes, then expand $PWD
    __load_env_value="${__load_env_value%\"}"
    __load_env_value="${__load_env_value#\"}"
    __load_env_value="${__load_env_value//\$PWD/$__load_env_repo_root}"

    export "$__load_env_name=$__load_env_value"
done <"$__load_env_repo_root/.env"

unset __load_env_repo_root __load_env_line __load_env_name __load_env_value
