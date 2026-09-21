platform ""
	requires {
		game : Game
	}
	exposes [Stdout, Stderr, Game, ShaderReflection, Graphs, RenderGraph]
	packages {}
	provides {
		"roc_init": init_for_host!,
		"roc_config": config_for_host,
		"roc_draw": draw_for_host,
	}
	hosted {
		"roc_stderr_line": Host.stderr_line!,
		"roc_stdin_line": Host.stdin_line!,
		"roc_stdout_line": Host.stdout_line!,
	}
	targets: {
		inputs_dir: "targets/",
		x64glibc: { inputs: ["Scrt1.o", "crti.o", "libhost.a", app, "crtn.o", "force_extract.o", "libstdc++.a", "libvulkan.so", "libm.so", "libc.so", "libc_forward.a", "libgcc_s.so"] },
	}

import Stdout
import Stderr
import Host
import Game
import ShaderReflection
import Graphs
import RenderGraph

## The return type is nominal so the generated glue names it. An anonymous
## record reaches Rust as a structural hash, and every field added to it
## renames the Rust type.
init_for_host! : {} => Game.Init
init_for_host! = |{}| Game.init!(game)

## Setup fetches definitions without calling app draw.
config_for_host : {} -> Game.HostConfig
config_for_host = |{}| game.host_config()

## Frames transfer only the selected graph ordinal and packed values.
draw_for_host : Game.Frame -> Graphs.Draw
draw_for_host = |frame| game.draw(frame)
