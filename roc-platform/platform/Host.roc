## Internal hosted-effect boundary used by the platform wrappers.
##
## Applications should import `Stdout` and `Stderr` instead.
Host := [].{
	stderr_line! : Str => Try({}, [StderrErr(Str)])
	stdout_line! : Str => Try({}, [StdoutErr(Str)])

	# NOTE: unused
	stdin_line! : {} => Try(Str, [StdinErr(Str)])
}
