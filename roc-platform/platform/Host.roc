## Internal hosted-effect boundary used by the platform wrappers.
##
## Applications should import `Stdout` and `Stderr` instead.
Host := [].{
	stderr_line! : Str => Try({}, [StderrErr(Str)])
	stdout_line! : Str => Try({}, [StdoutErr(Str)])

	## No platform module wraps it; the host still implements the hosted symbol.
	stdin_line! : {} => Try(Str, [StdinErr(Str)])
}
