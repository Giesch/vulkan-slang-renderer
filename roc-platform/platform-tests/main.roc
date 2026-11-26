app [game] { pf: platform "../platform/main.roc" }

import pf.Game

## Test host for the platform's render graph modules; `roc test` checks
## modules against this app's platform.
game = {
	init!,
}

init! : {} => Game.Init
init! = |_|
	{ window_title: "Render graph tests" }
