platform ""
	requires {
		init! : {} => InitConfig.Init
	}
	exposes [Stdout, Stderr, Stdin]
	packages {}
	provides { "roc_init": init_for_host! }
	hosted {
		"roc_stderr_line": Host.stderr_line!,
		"roc_stdin_line": Host.stdin_line!,
		"roc_stdout_line": Host.stdout_line!,
	}
	targets: {
		inputs_dir: "targets/",
		x64glibc: { inputs: ["Scrt1.o", "crti.o", "libhost.a", app, "crtn.o", "libstdc++.so", "libvulkan.so", "libm.so", "libc.so", "libc_nonshared.a", "libgcc_s.so"] },
	}

import Stdout
import Stderr
import Stdin
import Host
import InitConfig

## The return type is nominal so the generated glue names it. An anonymous
## record reaches Rust as a structural hash, and every field added to it
## renames the Rust type.
init_for_host! : {} => InitConfig
init_for_host! = |{}| InitConfig.new(init!({}))
