# 0004 — Strict GPU systems: the same `Float` bits on every device and on the CPU

- **Status:** draft
- **Depends on:** 0003

## Goal

A GPU system's `Float` results agree closely between devices, but not to
the last bit ("Determinism" in the reference lists how they may differ).
A simulation that must replay identically on another machine, or whose GPU
run must match its CPU run, cannot use one. After this spec it can say so:

```ruby
strict gpu system gravity
  each Body do |a|
    ax = 0.0
    ay = 0.0
    each Body do |b|
      dx = b.x - a.x
      dy = b.y - a.y
      d2 = dx * dx + dy * dy + SOFTENING
      pull = b.mass / (d2 * Math.sqrt(d2))
      ax += dx * pull
      ay += dy * pull
    end
    a.vx += ax * Time.dt
    a.vy += ay * Time.dt
    a.x += a.vx * Time.dt
    a.y += a.vy * Time.dt
  end
end
```

A strict GPU system gives the same bits on every device it runs on and on
the CPU. It costs more on the GPU; the default stays as it is.

Measurements on three drivers (an RTX 5090, Mesa's RADV on an RDNA 2
iGPU, and llvmpipe), summarised under "Measured in planning", showed that
this can be done with ordinary shader code.

## Scope

- `strict gpu system`: the declaration, its meaning in the reference, and
  what it may not contain.
- The meaning: `+`, `-` and `*` each rounded on their own, never fused or
  rearranged; `/` and `Math.sqrt` correctly rounded; subnormal numbers
  treated as zero, the same way on every target; comparisons and
  conversions defined to the bit.
- In the shader: an integer barrier on every operand of `+`, `-` and `*`,
  and every other `Float` operation as integer code on the bits.
- On the CPU: the same helpers, and flush-to-zero set while a strict block
  runs.
- A check at start-up that runs strict code on the device and the CPU and
  compares them; a device that fails runs strict blocks on the CPU.
- Tests that hold every strict program to its exact output on every
  device, and a test of `/` and `sqrt` on the device over a large input set
  (every input, for `sqrt`).
- A strict gravity benchmark beside the default one.
- The reference becomes Midnight 0.4 and the compiler 0.4.0.

## Non-goals

- `Math.sin`, `Math.cos`, `Math.tan`, `Math.atan2`, `Math.exp`,
  `Math.log` and `**` with a `Float` result in a strict system. They are a
  compile error there until a later spec gives Midnight its own
  implementations.
- Any change to ordinary code or to GPU systems that are not strict: same
  generated C, same shaders, same results, subnormals kept on the CPU.
- `strict` for ordinary systems, and the same 64-bit results on every
  platform for ordinary code (libm differs between platforms).
- A program-wide or build-flag form of `strict`.
- Integer or fixed-point arithmetic as a route to agreement.
- Verifying Metal, Intel GPUs or other NVIDIA and AMD generations. The
  start-up check covers them at run time; the acceptance criteria cover the
  three devices this machine reaches.
- AArch64. The flush-to-zero code is written for it and untested.

## Design

### The language

**Declaration.** `strict` goes before `gpu system`:

```
system = [ "strict" ] [ "gpu" ] "system" IDENT NL block "end"
```

`strict system name` without `gpu` is a compile error:

```
error: `strict` applies to a `gpu system`
```

with the hint ``write `strict gpu system name` `` in the label. Everything
a GPU system may and may not contain still holds.

**Meaning.** In a strict GPU system, and in a function while one calls it,
`Float` is the 32-bit `Float` of "Arithmetic at 32 bits" with these rules,
which hold on every device and on the CPU:

- **Subnormals count as zero.** An operand of `+`, `-`, `*`, `/`, `%`,
  `Math.sqrt`, a comparison or a conversion whose magnitude is below
  2 ** -126 (about 1.18e-38) is zero with its sign. A result of `+`, `-`,
  `*`, `/` or `Math.sqrt` whose exact value is below 2 ** -126 in
  magnitude is zero with its sign, even where rounding would have given
  2 ** -126. A value that is only copied, stored, passed or returned keeps
  its bits, and so does `-x` and `x.abs`, which change only the sign.
- **`+`, `-` and `*`** are each rounded to nearest, ties to even, on their
  own. They are never combined into one operation with a single rounding,
  and never rearranged: `a * b + b * c` is two products and a sum.
- **`/` and `Math.sqrt`** are correctly rounded, for every operand.
- **`%`** is the formula of "Arithmetic at 32 bits", every step under these
  rules.
- **`x.round(digits)`** is computed as at 32 bits today, every step under
  these rules.
- **Comparisons** are IEEE: with a NaN, `==`, `<`, `<=`, `>` and `>=` are
  false and `!=` is true. `-0.0 == 0.0`, and a subnormal equals zero.
  `min`, `max` and `clamp` follow from them.
- **`n.to_f`** is correctly rounded, ties to even.
- **`to_i`, `floor`, `ceil`, `round`** are as at 32 bits today, with a
  subnormal operand counting as zero.
- **`x ** 2.0`** is `x * x`. Every other `**` with a `Float` result is a
  compile error, as are `Math.sin`, `Math.cos`, `Math.tan`, `Math.atan2`,
  `Math.exp` and `Math.log`:

  ```
  error: `Math.sin` cannot be used in a strict GPU system
  ```

  with the hint `its result differs between devices`. In a
  function, the diagnostic carries the usual note naming the GPU system and
  the call that reaches it.
- **NaN.** Which NaN a result is cannot be seen, as today.

A function called from a strict system and from one that is not means the
strict thing in the first and the ordinary thing in the second, as the
same `def` already computes at 64 bits in ordinary code and at 32 in a GPU
block.

**Determinism** gains a bullet: a strict GPU system gives the same results
on every device and on the CPU, in both build modes. The `Float` rules of
the bullets above it apply to GPU systems that are not strict. "Not in
0.3", now "Not in 0.4", drops "results that are the same to the last bit
on every GPU and on the CPU" and gains "`Math` functions and `**` in a
strict GPU system".

**Without a GPU.** A strict block that runs on the CPU follows the same
rules, subnormals included. Ordinary code and blocks that are not strict
keep subnormals.

**The start-up check.** "Without a GPU" also says: when a program with a
strict GPU system finds a device, it first runs a fixed piece of strict
code on that device and on the CPU. If they differ, every strict block
runs on the CPU, with the same results, and the other GPU systems still
use the device. `MIDNIGHT_GPU=info` prints the outcome.

**`midnight plan`** prints `strict` before a strict system's name where it
names GPU systems. Plans of programs without one do not change.

### The compiler

**Front end.** `SystemDecl` (`src/ast.rs`) gains `strict: bool`; the
parser accepts `strict gpu system`. The checker reports the errors above,
and `src/tir.rs` carries `strict` on the system. Both contract files change
together with their two sides.

**Lowering** (`src/emit/gpu/lower.rs`). A function is lowered once per mode
that calls it, so a function used by both kinds of system becomes two
functions in C and in WGSL. In strict mode:

- **`+`, `-`, `*` on `F32`** stay hardware operations, and each operand is
  wrapped in a new IR node, `Opaque(side, expr)`. WGSL prints it as
  `bitcast<f32>(bitcast<u32>(x) ^ mn_z[side])`, the left operand with word
  0 and the right with word 1. C prints `x`: clang with
  `-ffp-contract=off` neither fuses nor rearranges. Every operand is
  wrapped, literals and loads included. A product feeding a sum passes
  through the xor, so the driver cannot fuse it, and two loads of the same
  value become two values the driver cannot prove equal, so it cannot
  cancel `x - x` or fold `x * 0.0`.
- **Everything else on `F32`** becomes helpers in `helpers.rs`, integer
  code on the bits, printed into both C and WGSL: negation and `abs`
  (the sign bit), the six comparisons, `floor`, `ceil` and `trunc`
  (`to_i`, `round` and `%` already build on them), `to_f` where range
  analysis cannot show that the `Int` is within ±2 ** 24, `/` and `sqrt`.
  Each follows the subnormal rules above.
- **`/` and `sqrt` in WGSL** are exact integer code. The recommended form
  takes the hardware result as an estimate, computes the exact residual of
  it and its neighbours with 16-bit limbs, and falls back to bit-by-bit
  long division (or square root) when the estimate is more than two units
  away. The hardware estimate then affects speed only, never the result.
  Slice 1 measures this against plain long division and keeps the faster
  one.
- **`/` and `sqrt` in C** are the hardware instructions, run with
  flush-to-zero set (see "The runtime"). They give the same bits as the
  helpers; the tests prove it.
- **No constant folding of `F32` arithmetic.** The operands are no longer
  constant expressions in WGSL, so nothing requires it, and Rust's `f32`
  does not flush.

**Shader.** A strict block's module binds one more read-only storage
buffer, `mn_z: array<u32, 2>`, which holds two zeros. It counts against the
device's storage-buffer limit; a block that no longer fits runs on the CPU
under the existing rule.

**C.** Each strict function begins with `#pragma STDC FENV_ACCESS ON`, so
clang does not fold a subnormal product of literals at compile time under
the default floating-point environment. A test with such a product holds
this in both build modes; if clang still folds, the C side wraps literal
operands instead.

### The runtime

**Flush-to-zero on the CPU.** `midnight_rt.h` gains `mn_strict_enter` and
`mn_strict_leave`, which save the floating-point control state, set
flush-to-zero and denormals-are-zero, and restore the saved state. On
x86-64 that is MXCSR bits 15 and 6, through `_mm_getcsr` and `_mm_setcsr`;
on AArch64, FPCR bit 24 (FZ). Generated C calls them around the CPU run of
each strict block. Both are compiler intrinsics, so `midnight_rt.c` stays
C11 with libc and libm.

**The zero buffer.** `midnight_gpu.c` creates `mn_z` once, 8 bytes of
zeros, and binds it to every strict block.

**The start-up check.** The compiler emits one more strict block into every
program with a strict GPU system: a fixed list of expressions over a fixed
table of inputs, chosen to fail where the barrier or the helpers do not
hold. It includes `a * b + c` at cancellation, `a * a + b * b`,
`a * b + b * c`, `x - x` and `x * 0.0` with infinities and NaN,
`x + 0.0` with -0, `/` and `sqrt` at hard cases and for divisors at or
above 2 ** 126, products in the subnormal boundary window, and comparisons
with NaN and subnormals. When the device is opened, the runtime runs it on
the device and on the CPU and compares the bits. A difference marks the
device as failing the check: every strict block runs on the CPU and
`MIDNIGHT_GPU=info` prints `strict blocks run on the CPU: <device> failed
the strict check`. `MIDNIGHT_GPU=strict-cpu` treats the device as failing,
for the tests.

### Measured in planning

16,673 rows per operation, three devices:

- `+`, `-` and `*` alone agree bit for bit on all three GPUs, and with the
  CPU once it flushes subnormals.
- All three GPUs rewrite arithmetic: the 5090 and RADV fuse multiply-add
  (choosing different products in `a * a + b * b`), and all three factor
  `a * b + b * c`. An xor with a zero read from a storage buffer at run
  time stopped both on every driver. A zero from a uniform also worked, but
  lavapipe can inline uniforms (Mesa MR 17722); a storage buffer is out of
  reach of that.
- `/` and `sqrt` are up to 2 ULP off on the 5090 and RADV, which disagree
  with each other on 15 to 25 % of rows, and both give 0 for divisors at or
  above 2 ** 126. A correction from the hardware result with exact Dekker
  residuals matched the CPU on every row it handled; the integer residual
  above is the same idea without depending on float rounding.
- `Math` functions agree nowhere.

The subnormal boundary:
128 products whose exact value lies just below 2 ** -126. The 5090,
llvmpipe, RADV and the Ryzen 9950X3D with FTZ and DAZ set all give zero
with the sign, the same bits on every row, including the 64 that would
round up to 2 ** -126. That is flushing before rounding, the rule the
language adopts. QEMU's maintainers model x86 FTZ as flushing after
rounding; Intel CPUs were not tried.

On Metal, `wgpu-hal` 27 compiles shaders with `MTLCompileOptions` whose
math mode it never sets, so Metal's default applies, historically fast
math. Not tried; the start-up check decides there.

Not measured: the cost. Gravity's inner loop has eleven `+`, `-` and `*`,
a division and a square root; strict adds two xors per operation and the
integer `/` and `sqrt`. Expect several times the GPU time of the default
on compute-bound blocks.

### Tests

- `tests/run/strict_*.12am` with `.out` from the CPU path, covering the
  start-up check's list and more: fused and factored shapes, cancellation,
  `x - x` and `x * 0.0` with infinities and NaN, -0, `/` and `sqrt` over
  the whole range, subnormal operands, results and the boundary window,
  comparisons, `min`, `max`, `clamp`, `to_f` of large `Int`s, `%`,
  `round(digits)`, a function called from a strict and a non-strict
  system, and `rand` in a strict system.
- `gpu_programs` holds every program whose GPU systems are all strict to
  its `.out` exactly. Such a program may not have a `# tolerance:` line;
  the test fails if it does.
- A new GPU-mode test, `gpu_strict_div_sqrt`, runs the WGSL of the strict
  `/` and `sqrt` on the device: `sqrt` for every one of the 2 ** 32 bit
  patterns, `/` for 2 ** 28 pairs (random bits, edge values, and quotients
  next to a midpoint). Every result
  must equal the CPU's `/` and `sqrtf` under FTZ and DAZ.
- `tests/fail`: `strict system`, each `Math` function and `**` in a strict
  system, and one reached through a function.
- `tests/plan`: one program with a strict and a non-strict GPU system.
- A run test with `MIDNIGHT_GPU=strict-cpu` that prints the same `.out`.
- `MIDNIGHT_TEST_GPU=hardware` keeps a `VK_DRIVER_FILES` the caller sets,
  so the suite can run on RADV here.
- `tests/COVERAGE.md` gains the new construct, diagnostics and rules.
- `bench/gpu_gravity_strict.12am`: `gpu_gravity` with `strict`.
  `bench-gpu.sh` runs it on the GPU and the CPU beside the default and
  prints both checksums.

### Slices

1. **Probe the cost.** Hand-written WGSL of strict gravity (barriers,
   integer `/` and `sqrt`, both forms) on the 5090 and llvmpipe, timed
   beside the default. If strict gravity is more than ten times the
   default on the 5090, stop and report before going on.
2. Front end: `strict`, diagnostics, `tir`, `plan`.
3. Lowering per mode, `Opaque`, the bit helpers, no folding; C pragma and
   `mn_strict_enter`/`leave`. Strict run tests pass on the CPU path.
4. WGSL `/` and `sqrt`, `mn_z`; `gpu_programs` exact on the three devices;
   `gpu_strict_div_sqrt`.
5. The start-up check and `strict-cpu`.
6. Benchmark, reference 0.4, README, COVERAGE, Outcome.

## Acceptance criteria

All run on x86_64-linux. "The three devices" are the RTX 5090, llvmpipe
and RADV on the Ryzen's iGPU (`VK_DRIVER_FILES` set to Mesa's
`radeon_icd.x86_64.json` from the dev shell).

1. `nix flake check` passes, and `nix run .#gpu-test` passes on the
   hardware device.
2. For every program of `tests/` and `examples/` that existed in 0.3.1,
   `midnight emit-c` and the generated shaders are byte for byte what 0.3.1
   gives, and every `.out`, `.err` and `.plan` file is unchanged.
3. Every strict program of `tests/run`, `tests/panic` and `examples` prints
   exactly its expected output on each of the three devices, in release and
   debug, as it is and with `MIDNIGHT_GPU=rows`, and with
   `MIDNIGHT_GPU=off`.
4. `gpu_strict_div_sqrt` passes on each of the three devices.
5. The start-up check passes on each of the three devices (`info` shows no
   strict block on the CPU). With `MIDNIGHT_GPU=strict-cpu`, every strict
   program prints the same output and `info` names the reason.
6. A strict program that multiplies two literals whose product is
   subnormal prints `0.0` (with its sign) in release and debug builds.
7. `bench/gpu_gravity_strict.12am` prints the same checksum on the 5090,
   llvmpipe, RADV and the CPU. The Outcome records its time per tick on
   the 5090 and on the CPU beside `gpu_gravity`'s, fastest of 5 on a quiet
   machine.
8. `nix run .#bench-gpu` and `nix run .#bench` show no row of 0.3.1's
   output more than 10 % slower.
9. Every diagnostic listed under "The language" has a `tests/fail`
   program, and `tests/COVERAGE.md` maps the new rules to tests.
10. `ldd` on the release build of `examples/hello.12am` lists what it
    lists on 0.3.1.
11. The reference says 0.4 and `midnight --version` 0.4.0, and every
    command in the README works as written.

## Decisions for review

- **Per system, as a keyword.** The source says which systems are strict,
  next to `gpu`; a program can keep a fast default for systems whose
  results only need to agree closely; one source means one thing, as a
  build flag would not.
- **The CPU path of a strict block flushes subnormals; ordinary code does
  not.** Agreement needs the flush; nothing else should change.
- **An unverified device is checked at start-up, not trusted or refused.**
  The barrier rests on driver behaviour that no specification promises.
  Running strict blocks on the CPU after a failed check keeps the
  guarantee, as a GPU system without a device already does.
- **`/` and `sqrt` as exact integer code** in the shader, not corrected
  float code: the result then never depends on the device's division,
  which on Metal may be worse than the 2 ULP seen here.
- **`%` stays the same formula**, now exact because its steps are.
- **Flush before rounding**, because all four targets measured do it.
- **`Math` functions and `**` are compile errors** in a strict system
  until a later spec writes them as helpers, which print to C and WGSL
  alike.
