import GraphSet
import ValidatedRenderGraph

## An app's initialization and draw callback, with its graph type erased.
Game :: { init! : {} => Init, config : HostConfig, draw : Frame -> GraphSet.Draw }.{

	## Static configuration provided to the host before drawing.
	## Nominal so the generated host glue gives it a stable Rust type name.
	HostConfig := { graphs : List(ValidatedRenderGraph.HostGraph) }

	## The record provided to `Game.new`.
	Definition(g) : {
		## called once at startup, before the window opens
		init! : {} => Init,

		## registered render graphs
		graphs : GraphSet.GraphSet(g),

		## the draw function called every frame
		draw : Frame -> GraphSet.Submission(g),
	}

	# These are nominal types so that they get picked up by the glue script

	## What an app's `init!` returns.
	Init := {
		window_title : Str,

		## Recompile shaders from `shaders_source_dir` when a source changes,
		## and swap in the rebuilt pipelines. `VKR_SHADER_HOT_RELOAD=1`/`=0`
		## overrides it at launch.
		shader_hot_reload : Bool,

		## The project's slang sources, usually
		## `ShaderAtlas.shaders_source_dir`. A relative path resolves against
		## the working directory, then the executable's directory; `""` or a
		## directory found in neither leaves hot reload off.
		shaders_source_dir : Str,
	}.{

		## A window titled `window_title`, with no shader sources to reload.
		titled : Str -> Init
		titled = |window_title| { window_title, shader_hot_reload: Bool.False, shaders_source_dir: "" }
	}

	## Data passed to `draw` every frame.
	Frame := {
		## aspect ratio of the window; width / height
		aspect_ratio : F32,

		## total time since the app opened (in seconds)
		elapsed : F32,
	}

	## Create a game to hand off to the platform.
	## The render graphs and draw function have to agree on the graph type,
	## and each frame's submission must come from `graphs` itself: drawing
	## from another set of the same type crashes.
	new : Definition(g) -> Game
	new = |definition| {
		graphs = definition.graphs
		config = HostConfig.({ graphs: graphs.definitions() })
		app_draw = definition.draw
		host_draw = |frame| graphs.accept_submission(app_draw(frame))

		Game.({ init!: definition.init!, config, draw: host_draw })
	}

	init! : Game => Init
	init! = |Game.(game)| (game.init!)({})

	host_config : Game -> HostConfig
	host_config = |Game.(game)| game.config

	draw : Game, Frame -> GraphSet.Draw
	draw = |Game.(game), frame| (game.draw)(frame)
}
