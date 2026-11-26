## Thermostat

A thermostat keeps a room warm with three heaters. Temperatures are whole
numbers of degrees. The room starts at 10 degrees; the thermostat's set
point is 21 degrees and the air outside is at 5 degrees.

| Heater | Power (degrees per tick) | Burns out                    |
| ------ | ------------------------ | ---------------------------- |
| A      | 3                        | never                        |
| B      | 2                        | never                        |
| C      | 4                        | after it has been on 4 ticks |

The program runs 15 ticks. Each tick has three steps, in this order:

1. **Heating.** The thermostat looks at the room's temperature once, at
   the start of the tick. If it is below the set point, every heater that
   has not burned out is on for this tick and adds its power to the room's
   temperature. If it is at or above the set point, every heater is off for
   this tick. A heater that has been on for 4 ticks in all has burned out
   and is never on again; heater C burns out this way, and A and B never do.
2. **Cooling.** The room then loses heat to the outside: its temperature
   goes down by (temperature - 5) divided by 5, rounded down to a whole
   number, where temperature is the value after heating.
3. **Report.** The program prints one line:
   `tick T: D degrees, heaters on: N, energy: E`, where `T` is the tick's
   number, counting from 1; `D` is the temperature after cooling; `N` is the
   number of heaters that were on during this tick; and `E` is the total
   power the heaters have added to the room since the start, this tick
   included.

The program prints exactly 15 lines and nothing else. The first three are:

```
tick 1: 17 degrees, heaters on: 3, energy: 9
tick 2: 22 degrees, heaters on: 3, energy: 18
tick 3: 19 degrees, heaters on: 0, energy: 18
```
