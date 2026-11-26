## League

Six teams play a league: Bees, Ants, Cats, Dogs, Eels and Fish. Every team
plays every other team once, over five rounds. The program plays one round
per tick, in this order:

| Round | Matches (home team, home goals - away goals, away team) |
| ----- | ------------------------------------------------------- |
| 1     | Bees 2-1 Ants, Cats 0-0 Dogs, Eels 1-3 Fish             |
| 2     | Bees 1-3 Cats, Ants 2-2 Eels, Dogs 1-0 Fish             |
| 3     | Bees 0-1 Dogs, Ants 1-0 Fish, Cats 2-2 Eels             |
| 4     | Bees 1-1 Eels, Ants 0-2 Dogs, Cats 1-0 Fish             |
| 5     | Bees 3-0 Fish, Ants 1-1 Cats, Dogs 0-1 Eels             |

- A team that scores more goals than its opponent wins and gets 3 points.
  In a draw (equal goals) each team gets 1 point. A loss gets 0 points.
- A team's goal difference is the number of goals it has scored minus the
  number of goals scored against it, over all its matches so far, home and
  away.

Each team is an entity. The program has three systems, which run once a
tick in this order: `announce` announces the round's matches as events,
`score` updates the teams from those events, and `table` prints the
round's line and the table.

After each round the program prints the round's results and then the
league table:

- First one line: `round N: ` followed by that round's three matches, in
  the order the table above lists them, each written as in the table
  (home team, a space, home goals, `-`, away goals, a space, away team),
  separated by a comma and a space. `N` counts from 1.
- Then one line per team, six in all, for the table after that round: its
  place, a full stop, a space, its name, a space, its points, a space and
  its goal difference, as in `2. Bees 3 1`. A positive goal difference is
  printed with no sign, a negative one with a minus sign, and zero as `0`.

The table orders teams by points, most first. Teams with equal points are
ordered by goal difference, highest first. Teams with equal points and
equal goal difference are ordered by name, alphabetically. Places are
numbered 1 to 6 from the top, and no two teams share a place.

The program prints exactly 35 lines and nothing else. The first seven are:

```
round 1: Bees 2-1 Ants, Cats 0-0 Dogs, Eels 1-3 Fish
1. Fish 3 2
2. Bees 3 1
3. Cats 1 0
4. Dogs 1 0
5. Ants 0 -1
6. Eels 0 -2
```
