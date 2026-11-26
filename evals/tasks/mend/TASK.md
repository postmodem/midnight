## Mend

The file `solution.12am` in this directory counts the ballots of a vote
for the village tree and prints the results. It was written by a Ruby
programmer new to Midnight, the way they would write it in Ruby, and it
does not compile. Fix it, so that `midnight run solution.12am` prints
exactly what is described below. Keep the ballots as they are in the
program; change how the program does its work, not what it prints.

The program prints, in this order:

1. A line `ballots:`.
2. The ballots, in the order they are listed in the program, six to a
   line, separated by one space. The last line has the ballots that are
   left over. No line has a space at its start or its end.
3. A line `results:`.
4. One line for each tree that got at least one vote. The trees are
   ranked by their number of votes, most first; trees with the same number
   of votes are in alphabetical order. Each line is:
   - the tree's name, padded with spaces on the right to 6 characters;
   - one space;
   - its number of votes, padded with spaces on the left to 2 characters;
   - one space;
   - its share of all the ballots, in percent: its number of votes
     times 100, divided by the number of ballots, rounded to one decimal
     place and always written with one digit after the decimal point,
     padded with spaces on the left to 5 characters;
   - a `%` sign and one space;
   - one `#` for each of its votes.
5. A line `winner: NAME`, with the name of the first tree in the ranking.
6. A line `margin: N`, where `N` is the first tree's number of votes minus
   the second tree's.

The program prints nothing else. Its output begins:

```
ballots:
oak ash elm ash elm oak
ash birch oak ash ash elm
ash yew oak yew birch birch
elm elm ash elm ash oak
oak elm oak oak yew elm
ash yew yew elm oak oak
elm elm elm ash birch
results:
elm    12  29.3% ############
```
