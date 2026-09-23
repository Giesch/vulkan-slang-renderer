import Graphs
import ValidatedRenderGraph

## An app's initialization and draw callback, with its graph type erased.
Game :: { init! : {} => Init, config : HostConfig, draw : Frame -> Graphs.Draw }.{
	Config : { window_title : Str }

	## Static configuration provided to the host before drawing.
	## Nominal so the generated host glue gives it a stable Rust type name.
	HostConfig := { graphs : List(ValidatedRenderGraph.HostGraph) }

	## The record provided to `Game.new`
	Definition(g) : {

		## called once at startup
		init! : {} => Init,

		## registered render graphs
		graphs : Graphs.Graphs(g),

		## the draw function called every frame
		draw : Frame -> Graphs.Submission(g),
	}

	# These are nominal types so that they get picked up by the glue script

	## What an app's `init!` returns.
	Init := { window_title : Str }

	## Data passed to `draw` every frame
	Frame := {

		## aspect ratio of the window; width / height
		aspect_ratio : F32,

		## total time since the app opened (in seconds)
		elapsed : F32,
	}

	## Create a game to hand off to the platform.
	## The render graphs and draw function have to agree on the graph type.
	new : Definition(g) -> Game
	new = |{ init!, graphs, draw }| {
		config = HostConfig.({ graphs: graphs.definitions() })
		host_draw = |frame| draw(frame).to_host()

		Game.({ init!, config, draw: host_draw })
	}

	init! : Game => Init
	init! = |Game.(game)| (game.init!)({})

	host_config : Game -> HostConfig
	host_config = |Game.(game)| game.config

	draw : Game, Frame -> Graphs.Draw
	draw = |Game.(game), frame| (game.draw)(frame)
}
