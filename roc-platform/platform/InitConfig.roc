## What the host reads before it opens the window.
##
## Internal to the platform. An app returns a `Game.Init` from `init!`, and
## `main.roc` wraps it here.
InitConfig := { window_title : Str }.{

	## What an app's `init!` returns.
	Init : { window_title : Str }

	## Wrap the record an app's `init!` returns.
	new : Init -> InitConfig
	new = |{ window_title }| InitConfig.{ window_title }
}
