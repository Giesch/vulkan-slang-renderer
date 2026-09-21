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

	## What an app's `init!` returns.
	## Nominal so the generated host glue gives it a stable Rust type name.
	Init := { window_title : Str }

	## What the host passes to `draw`: the window's width divided by its height.
	## Nominal so the generated host glue gives it a stable Rust type name.
	Frame := { aspect_ratio : F32 }

	## The collection and draw callback must agree on the registered graph type.
	## The app uses its own collection; only the host closure erases its type.
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
