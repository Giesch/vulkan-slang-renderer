app [game] { pf: platform "../platform/main.roc" }

import pf.Game
import pf.GraphSet
import pf.RenderGraph
import pf.ResourcePlanner

## Test host for the platform's render graph modules; `roc test` checks
## modules against this app's platform.
game = Game.new({ init!, graphs, draw })

Ok(graphs) = RenderGraph.new(ResourcePlanner.empty, RenderGraph.list([])).map_ok(GraphSet.single)

init! : {} => Game.Init
init! = |_| Game.Init.titled("Render graph tests")

draw = |_frame| graphs.draw([])
