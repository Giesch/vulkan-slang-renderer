platform ""
	requires {
		init! : {} => InitConfig
	}
	exposes [Stdout, Stderr, Stdin, InitConfig]
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

init_for_host! : {} => InitConfig
init_for_host! = |{}| init!({})
