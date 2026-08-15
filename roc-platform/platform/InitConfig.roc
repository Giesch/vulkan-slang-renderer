## What the host reads before it opens the window.
InitConfig := { window_title : Str }.{

	## Build the value an app's `init!` must return.
	new : Str -> InitConfig
	new = |window_title| InitConfig.{ window_title }
}
