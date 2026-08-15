## What the host reads before it opens the window.
##
## Internal to the platform. An app returns a plain record from `init!`, and
## `main.roc` wraps it here.
InitConfig := { window_title : Str }.{

	## Wrap the record an app's `init!` returns.
	new : { window_title : Str } -> InitConfig
	new = |{ window_title }| InitConfig.{ window_title }
}
