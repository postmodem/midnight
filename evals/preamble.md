# Task

This task is a program to write in Midnight, a programming language.
Midnight's documentation is in `docs/`.

The compiler is on the PATH:

- `midnight check FILE` reports errors without running anything.
- `midnight run FILE` compiles the program and runs it.

The program goes in `solution.12am` in this directory. Before you finish,
run it with `midnight run solution.12am` and make sure it prints exactly
what the task asks. Nothing outside this directory is available: there is
no network, and no other files or programs can be read or run. Any other
file you write, such as a file of commands to try the program with, goes
in this directory too.

Write and change files with the file tools, not the shell. The shell runs
`midnight` commands and commands that only read files; give each
`midnight` command on its own, not joined to other commands. Any other
command is refused. A refused command does not end the task: carry on
with the tools that work.
