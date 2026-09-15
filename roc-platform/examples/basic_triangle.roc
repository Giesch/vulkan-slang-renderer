app [game] { pf: platform "../platform/main.roc" }

import pf.Game

game = {
	init!,
}

init! : {} => Game.Init
init! = |_| { window_title: "Basic Triangle from Roc!" }
