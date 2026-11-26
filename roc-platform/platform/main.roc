platform ""
	requires {
		game : Game
	}
	exposes [Game, GraphSet, RasterState, RenderGraph, ResourcePlanner, ShaderReflection, Stderr, Stdout]
	packages {}
	provides {
		"roc_config": config_for_host,
		"roc_draw": draw_for_host,
		"roc_init": init_for_host!,
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
import ResourcePlanner
import RenderGraph
import GraphSet
import RasterState

## Runs the app's `init!` once, before the host opens the window.
init_for_host! : {} => Game.Init
init_for_host! = |{}| Game.init!(game)

## Setup fetches the graph definitions without calling the app's `draw`.
config_for_host : {} -> Game.HostConfig
config_for_host = |{}| game.host_config()

## Frames transfer only the selected graph id and its frame values.
draw_for_host : Game.Frame -> GraphSet.Draw
draw_for_host = |frame| game.draw(frame)
