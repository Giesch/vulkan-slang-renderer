## Everything an app needs to write `init!`.
import InitConfig

Game := [].{
	Config : { window_title : Str }

	## What an app's `init!` returns.
	Init : InitConfig.Init
}
