#!/usr/bin/env sh

# allows the hook to work in magit
# https://docs.magit.vc/magit/FAQ-_002d-Issues-and-Errors.html#My-Git-hooks-work-on-the-command_002dline-but-not-inside-Magit
unset GIT_LITERAL_PATHSPECS

just pre-commit
