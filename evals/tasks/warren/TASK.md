## Warren

Rabbits and foxes live in a meadow of 12 places in a row, numbered 0 to 11.
Every animal has a number, and rabbits and foxes are numbered separately.
Rabbits never move. Foxes move as described below.

At the start there are ten rabbits and three foxes:

| Rabbit | Place | Age (ticks) |
| ------ | ----- | ----------- |
| 8      | 5     | 2           |
| 2      | 0     | 1           |
| 9      | 10    | 3           |
| 1      | 5     | 3           |
| 10     | 7     | 2           |
| 4      | 10    | 0           |
| 6      | 5     | 4           |
| 3      | 6     | 0           |
| 7      | 9     | 1           |
| 5      | 11    | 2           |

| Fox | Place | Hunger | Meals |
| --- | ----- | ------ | ----- |
| 2   | 1     | 0      | 1     |
| 1   | 5     | 0      | 0     |
| 3   | 8     | 0      | 0     |

Step 2 below says what a fox's hunger and meals do.

The program runs 15 ticks. Each tick has four steps, in this order. Each
step finishes for every animal before the next step begins.

1. **Hunt.** All the foxes hunt at the same time. Each fox picks the
   lowest-numbered rabbit at its own place, if there is one. When two or
   more foxes pick the same rabbit, the lowest-numbered of those foxes eats
   it, and the others eat nothing this tick: they do not pick again. A
   rabbit that is eaten is gone.
2. **Foxes.** For each fox:
   - A fox that ate this tick has its hunger set back to 0 and counts one
     more meal. When its meal count reaches 2, it has one cub and its meal
     count goes back to 0. The cub starts at its parent's place, with a
     hunger of 0 and no meals. A cub born in this step takes no further
     part in it: its hunger does not go up and it does not move.
   - A fox that did not eat this tick has its hunger go up by 1. If its
     hunger is now 3, it starves and is gone. Otherwise it moves 2 places
     along the row: from place `p` to place `p + 2`, except that from place
     10 it goes to place 0 and from place 11 to place 1.
3. **Rabbits.** For each rabbit, count the rabbits at its place when this
   step begins, itself included. A rabbit that is at least 2 ticks old when
   this step begins, and whose count is 3 or fewer, has one baby at its
   place. A baby is 0 ticks old. Babies born in this step are in no
   rabbit's count and have no babies of their own in this step. Then every
   rabbit that was there when this step began becomes one tick older;
   babies born in this step do not.
4. **Report.** The program prints one line:
   `tick T: R rabbits, F foxes, E eaten, B born, C cubs, S starved`, where
   `T` is the tick's number, counting from 1; `R` and `F` are the numbers of
   rabbits and foxes alive at the end of the tick, newborns included; and
   `E`, `B`, `C` and `S` are the numbers of rabbits eaten, rabbits born,
   cubs born and foxes starved during this tick. The words are always as
   shown, whatever the numbers.

A new animal gets the next number of its kind: one more than the highest
number any rabbit (for a baby) or fox (for a cub) has had so far. When
several are born in the same step, they are numbered in the order of their
parents' numbers, lowest first.

The program prints exactly 15 lines and nothing else. The first line is:

```
tick 1: 14 rabbits, 3 foxes, 1 eaten, 5 born, 0 cubs, 0 starved
```
