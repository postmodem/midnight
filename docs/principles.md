# Principles

Midnight is a language for games and simulations, written mostly by AI
agents and read by agents and people. These principles come before any
single spec: a spec says how it serves them, and a change that works
against one says so and why.

## Agents write, people review

Most Midnight code is written by agents, through agentic engineering, and
reviewed by people. So:

- Reading is optimized for both. A program says what it does in the
  order it happens, without hidden behavior, and one person can review an
  agent's program without running it in their head.
- Writing is optimized for agents. Where the two pull apart, the form an
  agent writes correctly the first time wins over the form a person types
  fastest.
- There is one obvious way to write a thing. Two spellings for the same
  meaning are two things an agent must learn and a reviewer must check.
- Explicit beats clever. A longer form that cannot be misread is better
  than a shorter one that can.

An agent learns Midnight from its documentation in every session, with no
memory of earlier programs. The documentation is the language's main
interface, and it is written for that reader: rules an agent can follow,
guidance on when to use a construct and when not, and complete examples.
Its size is a cost paid at the start of every session, and grows only for
something a program needs.

## The domain is in the language

What games and simulations are made of (entities, components, systems,
queries, resources, events, ticks, a window, input, the GPU) is part of the
language, not a library. The compiler sees all of it, which is what lets it
lay data out well, decide where loops run, plan what moves between CPU and
GPU, and check what a library could not.

A thing belongs in the language when the compiler can exploit it or check
it. Anything else belongs in code written in Midnight, which an agent reads
only when a program uses it.

## Fast by construction

Speed comes from the shape of the language, not from annotations:

- Data is laid out as a data-oriented C program would lay it out by hand.
- The compiler decides what can run on several cores or on the GPU, from
  what the program says, and never changes what it computes to go faster.
- What a construct costs is written down, in the reference and in what
  `midnight plan` and `midnight loops` print.

## Observable and repeatable

A program gives the same output every time, on every machine, for any
number of threads, and on any GPU within what the reference allows. An
agent checks its work by comparing output, and that only works when output
is exact.

The compiler explains its decisions: `midnight plan` says what moves
between CPU and GPU and `midnight loops` says where each loop runs and why.
A program with a window runs headless and is driven by commands, so an
agent can step it, give it input and look at its frames without a desktop.

## Errors teach

An error says what is wrong in one line and, where it can, what to write
instead. Fixing it should take one step. An error that agents keep running
into is a defect in the language or in its documentation, not in the
agents.

## The syntax is Midnight's own

The initial syntax borrowed from Ruby where Ruby reads well. It is still
being discovered: it changes as programs and measurement show what works
for agents and for people, and nothing borrowed stays only because Ruby
has it.

- A change to the syntax is made on evidence: programs that are hard to
  write correctly, hard to read, or mistakes agents make again and again.
  Nothing changes only to look different.
- A construct either means what it means in Ruby or looks clearly unlike
  Ruby. Code that looks like Ruby but behaves differently is the worst
  case, for an agent writing it and for a person reviewing it.
- Before 1.0, a better syntax is worth a breaking change.

## Grown by building things in it

Midnight grows from the programs written in it: small simulations and
games first, then larger ones. What those programs need, or find hard,
chooses the next spec, and each spec is checked against programs that
use it. A feature waits until a program wants it, and a feature that
programs want gets built in depth, not as a token.

## Built through agentic engineering

Midnight itself is built the way its programs are: by agents, from specs a
person approves.

- Every change starts from a spec with acceptance criteria, reviewed
  adversarially before a person reads it, and ends with an Outcome that
  records what was measured and where it differs from the design.
- Expected output is a claim about the language, and every construct,
  diagnostic and run-time error has a test.
- Every tool works from the command line with output an agent can use.
  Integrations built on top of it add convenience, never a capability the
  command line lacks.
- Anything an agent runs that could reach a desktop runs headless or in
  a compositor of its own.
