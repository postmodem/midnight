## Sandpile

A sandpile is a square grid of 7 rows and 7 columns of cells. Rows are
numbered 0 to 6 from the top and columns 0 to 6 from the left. Each cell
holds a whole number of grains of sand. At the start the cell in row 5,
column 1 holds 30 grains, and every other cell is empty. The pile settles
for the first time after grain 1 is dropped; its topples and lost grains
count from the start.

Grains are dropped on the grid one at a time, 80 in all. Grains 1 to 50
are each dropped on the cell in row 3, column 3. Grains 51 to 80 are each
dropped on the cell in row 1, column 4.

After each grain is dropped, the pile settles before the next grain:

- A cell that holds 4 or more grains topples: it loses 4 grains, and each
  of its four neighbours (the cells directly above, below, to the left and
  to the right) gains one. A grain that would go to a neighbour outside the
  grid is lost.
- A cell that still holds 4 or more grains after toppling topples again,
  and a neighbour that reaches 4 topples in its turn. The pile has settled
  when no cell holds 4 or more grains.
- Each time a cell gives away 4 grains counts as one topple. The number of
  topples and the grid after settling do not depend on the order in which
  cells topple.

After every tenth grain has been dropped and the pile has settled, the
program prints one line:
`after N grains: T topples, L lost`, where `N` is the number of grains
dropped so far, `T` is the number of topples since the start, and `L` is
the number of grains lost since the start. The words are always as shown,
whatever the numbers.

After the last of these lines the program prints the grid: one line per
row, from row 0 to row 6, each with the number of grains in the row's
cells, from column 0 to column 6, separated by single spaces.

The program prints exactly 15 lines and nothing else. The first three are:

```
after 10 grains: 23 topples, 8 lost
after 20 grains: 31 topples, 8 lost
after 30 grains: 55 topples, 10 lost
```
