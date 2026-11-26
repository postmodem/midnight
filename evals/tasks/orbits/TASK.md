## Orbits

Two bodies, A and B, move in a plane under each other's gravity. Each has
a mass, a position (x, y) and a velocity (vx, vy). All of them are 64-bit
floating-point numbers, and so is every value computed from them. At the
start:

| Body | Mass | x   | y   | vx  | vy   |
| ---- | ---- | --- | --- | --- | ---- |
| A    | 8    | 0   | 0   | 0   | -0.2 |
| B    | 1    | 4   | 0   | 0   | 1.6  |

Time moves in steps of `dt = 1/16`. The program runs 640 steps. In each
step, with every value taken as it is at the start of the step:

```
dx = B.x - A.x
dy = B.y - A.y
d2 = dx * dx + dy * dy
s  = d2 * sqrt(d2)
```

where `sqrt` is the square root. Then the velocities change, all four from
these same `dx`, `dy` and `s`:

```
A.vx = A.vx + B.mass * dx / s * dt
A.vy = A.vy + B.mass * dy / s * dt
B.vx = B.vx - A.mass * dx / s * dt
B.vy = B.vy - A.mass * dy / s * dt
```

and then the positions, each from the new velocity:

```
A.x = A.x + A.vx * dt
A.y = A.y + A.vy * dt
B.x = B.x + B.vx * dt
B.y = B.y + B.vy * dt
```

Each formula is computed exactly as written, operations from left to
right except that `*` and `/` come before `+` and `-`, so that every
correct program gets the same numbers to the last bit.

After every 32nd step, the program prints one line:
`t=T A(AX, AY) B(BX, BY)`, where `T` is the time so far (the number of
steps times `dt`) written with exactly one digit after the decimal point,
and `AX`, `AY`, `BX`, `BY` are the bodies' positions. Each position is
rounded to three decimal places, a half rounding away from zero, and
written with at least one digit before the decimal point and exactly three
after it, with a minus sign if it is negative. A position that rounds to
zero is written `0.000`, with no sign.

The program prints exactly 20 lines and nothing else. The first three are:

```
t=2.0 A(0.118, -0.367) B(3.059, 2.933)
t=4.0 A(0.373, -0.596) B(1.014, 4.769)
t=6.0 A(0.652, -0.691) B(-1.213, 5.529)
```
