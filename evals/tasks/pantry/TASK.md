## Pantry

A pantry keeps nine goods. Each good has a name, a kind and a stock, a
whole number of units. There are three kinds of good: `Grain`, `Dairy` and
`Fruit`. At the start the pantry holds:

| Good   | Kind  | Stock |
| ------ | ----- | ----- |
| pears  | Fruit | 6     |
| milk   | Dairy | 5     |
| rice   | Grain | 9     |
| apples | Fruit | 4     |
| oats   | Grain | 3     |
| butter | Dairy | 4     |
| figs   | Fruit | 0     |
| flour  | Grain | 7     |
| cheese | Dairy | 2     |

The program runs four days, one day per tick. Each day, deliveries arrive
and then orders come in, as listed here. Within a day, deliveries and
orders are taken in the order listed, from left to right.

| Day | Deliveries (good amount)    | Orders (good amount)                                                     |
| --- | --------------------------- | ------------------------------------------------------------------------ |
| 1   | figs 3                      | rice 4, milk 2, cheese 3, pears 1, oats 3                                |
| 2   | oats 5, cheese 2            | apples 4, butter 1, flour 2, figs 5, milk 3, rice 2, rice 4              |
| 3   | milk 6, apples 2, rice 3    | pears 2, cheese 4, milk 6, apples 3, butter 3, oats 1                    |
| 4   | butter 2, pears 1           | flour 5, figs 1, rice 3, oats 4, cheese 1, butter 2                      |

- A delivery adds its amount to the good's stock.
- An order is filled when the good's stock is at least the amount ordered:
  the amount is taken from the stock and counts as sold. Otherwise the
  order is refused, and nothing is taken or sold. An order is never filled
  in part.

Each good is an entity. The program has four systems, which run once a
tick in this order: `announce` announces the day's deliveries and orders
as events, `deliver` adds each delivery to its good's stock, `sell` fills
or refuses each order, and `report` goes over every good and prints the
day's lines.

At the end of each day the program prints:

- First one line: `day N: F filled, R refused`, where `N` is the day's
  number, counting from 1, and `F` and `R` are the numbers of orders filled
  and refused that day.
- Then one line for each kind, in the order `Grain`, `Dairy`, `Fruit`:
  the kind's name, a space, `(S sold)` where `S` is the number of units of
  that kind sold that day, a colon, a space, and then the goods of that
  kind, each as its name, a space and its stock at the end of the day,
  separated by a comma and a space, as in
  `Dairy (2 sold): butter 4, milk 3, cheese 2`.

Within a kind's line, goods with more stock come first; goods with equal
stock are ordered by name, alphabetically.

The words are always as shown, whatever the numbers. The program prints
exactly 16 lines and nothing else. The first four are:

```
day 1: 4 filled, 1 refused
Grain (7 sold): flour 7, rice 5, oats 0
Dairy (2 sold): butter 4, milk 3, cheese 2
Fruit (1 sold): pears 5, apples 4, figs 3
```
