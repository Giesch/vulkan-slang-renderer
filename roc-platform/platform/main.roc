platform ""
	requires {
		game : {
			init! : {} => Game.Init,
		}
	}
	exposes [Stdout, Stderr, Stdin, Game]
	packages {}
	provides { "roc_init": init_for_host! }
	hosted {
		"roc_stderr_line": Host.stderr_line!,
		"roc_stdin_line": Host.stdin_line!,
		"roc_stdout_line": Host.stdout_line!,
	}
	targets: {
		inputs_dir: "targets/",
		x64glibc: { inputs: ["Scrt1.o", "crti.o", "libhost.a", app, "crtn.o", "libstdc++.a", "libvulkan.so", "libm.so", "libc.so", "libc_nonshared.a", "libgcc_s.so"] },
	}

import Stdout
import Stderr
import Stdin
import Host
import InitConfig
import Game

## The return type is nominal so the generated glue names it. An anonymous
## record reaches Rust as a structural hash, and every field added to it
## renames the Rust type.
init_for_host! : {} => InitConfig
init_for_host! = |{}| InitConfig.new((game.init!)({}))
