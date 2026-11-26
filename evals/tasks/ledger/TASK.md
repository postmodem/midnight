## Ledger

A bank pays off six loans month by month and prints how they stand. The
bank's numbers must be the same on every machine that runs the program,
whatever its GPU, so the loans are worked out by a strict GPU system named
`amortize`. Each loan is an entity, and each tick is one month.

| Loan | Principal | Yearly rate | Term (months) |
| ---- | --------- | ----------- | ------------- |
| 3    | 25000     | 0.06        | 60            |
| 6    | 600       | 0.18        | 6             |
| 1    | 1800      | 0.12        | 12            |
| 5    | 135000    | 0.0425      | 360           |
| 2    | 4500      | 0.085       | 36            |
| 4    | 10000     | 0.0725      | 24            |

A loan's principal, rate, balance and monthly payment are 32-bit Floats,
stored as the table gives them, and every step below is worked out in
`amortize`, as a strict GPU system computes, in the order written.
`a * b * c / d` means: `a` times `b`, that times `c`, and that divided by
`d`.

In every tick, `amortize` does this for each loan:

1. `r` = rate / 12, the monthly rate.
2. In the first tick only, it works out the monthly payment. `f` starts at
   1 and is multiplied by (1 + `r`) once for each month of the term:
   `f` = `f` * (1 + `r`), as many times as the term has months. Then the
   payment = principal * `r` * `f` / (`f` - 1), and the balance starts
   at the principal.
3. If the loan has months of its term left, one month passes:
   `interest` = balance * `r`, then balance = balance + `interest` -
   payment (the interest added first, then the payment taken away), and
   the loan has one month fewer left. A loan with no months left does not
   change: its balance stays as it was, close to 0 but not always 0.

The program runs 48 ticks. After the first tick, and after ticks 12, 24,
36 and 48, it prints a report:

- `month M`, where `M` is the number of ticks run so far.
- Then one line for each loan, in the order of the loans' numbers, 1 to
  6: `loan N: pays P, owes B`, where `P` is the monthly payment and `B`
  the balance, each in whole cents. The cents are worked out where the
  line is printed, not in `amortize`: the amount times 100, as a 64-bit
  Float, rounded to the nearest whole number (no amount is ever exactly
  half a cent). A negative number is printed with a minus sign, as in
  `owes -3`.

The program prints exactly 35 lines and nothing else. The first seven are:

```
month 1
loan 1: pays 15993, owes 165807
loan 2: pays 14205, owes 438982
loan 3: pays 48332, owes 2464168
loan 4: pays 44886, owes 961156
loan 5: pays 66412, owes 13481402
loan 6: pays 10532, owes 50368
```
