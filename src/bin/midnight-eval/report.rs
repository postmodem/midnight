//! Statistics, the Markdown report of one results file, and two results
//! files side by side.

use std::cmp::Ordering;

use crate::draws;
use crate::json::Value;

// Statistics.

/// The mean of numbers; anything else counts as 0.
pub fn mean(xs: &[&Value]) -> Option<f64> {
    let xs: Vec<f64> = xs.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect();
    fmean(&xs)
}

pub fn fmean(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        None
    } else {
        Some(xs.iter().sum::<f64>() / xs.len() as f64)
    }
}

/// The standard error of the mean.
pub fn se(xs: &[f64]) -> Option<f64> {
    if xs.len() < 2 {
        return None;
    }
    let m = fmean(xs)?;
    let ss: f64 = xs.iter().map(|x| (x - m) * (x - m)).sum();
    Some((ss / (xs.len() - 1) as f64).sqrt() / (xs.len() as f64).sqrt())
}

/// A whole number of any size.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Big(Vec<u32>);

impl Big {
    fn from(n: u64) -> Big {
        let mut b = Big(vec![n as u32, (n >> 32) as u32]);
        b.trim();
        b
    }

    fn trim(&mut self) {
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
    }

    fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    fn mul_small(&self, m: u32) -> Big {
        let mut out = Vec::with_capacity(self.0.len() + 1);
        let mut carry = 0u64;
        for &d in &self.0 {
            let v = d as u64 * m as u64 + carry;
            out.push(v as u32);
            carry = v >> 32;
        }
        out.push(carry as u32);
        let mut b = Big(out);
        b.trim();
        b
    }

    fn div_small(&self, m: u32) -> Big {
        let mut out = vec![0u32; self.0.len()];
        let mut rem = 0u64;
        for i in (0..self.0.len()).rev() {
            let v = (rem << 32) | self.0[i] as u64;
            out[i] = (v / m as u64) as u32;
            rem = v % m as u64;
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    fn mul(&self, o: &Big) -> Big {
        let mut out = vec![0u64; self.0.len() + o.0.len() + 1];
        for (i, &a) in self.0.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in o.0.iter().enumerate() {
                let v = out[i + j] + a as u64 * b as u64 + carry;
                out[i + j] = v & 0xffff_ffff;
                carry = v >> 32;
            }
            let mut k = i + o.0.len();
            while carry > 0 {
                let v = out[k] + carry;
                out[k] = v & 0xffff_ffff;
                carry = v >> 32;
                k += 1;
            }
        }
        let mut b = Big(out.into_iter().map(|v| v as u32).collect());
        b.trim();
        b
    }

    fn add(&self, o: &Big) -> Big {
        let n = self.0.len().max(o.0.len());
        let mut out = Vec::with_capacity(n + 1);
        let mut carry = 0u64;
        for i in 0..n {
            let v = *self.0.get(i).unwrap_or(&0) as u64 + *o.0.get(i).unwrap_or(&0) as u64 + carry;
            out.push(v as u32);
            carry = v >> 32;
        }
        out.push(carry as u32);
        let mut b = Big(out);
        b.trim();
        b
    }

    /// self - o, for o <= self.
    fn sub(&self, o: &Big) -> Big {
        let mut out = Vec::with_capacity(self.0.len());
        let mut borrow = 0i64;
        for i in 0..self.0.len() {
            let mut v = self.0[i] as i64 - *o.0.get(i).unwrap_or(&0) as i64 - borrow;
            borrow = 0;
            if v < 0 {
                v += 1 << 32;
                borrow = 1;
            }
            out.push(v as u32);
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    fn bits(&self) -> usize {
        match self.0.last() {
            None => 0,
            Some(&top) => (self.0.len() - 1) * 32 + (32 - top.leading_zeros() as usize),
        }
    }

    fn bit(&self, i: usize) -> bool {
        self.0.get(i / 32).is_some_and(|d| d >> (i % 32) & 1 == 1)
    }

    fn shl1_or(&self, bit: bool) -> Big {
        let mut out = Vec::with_capacity(self.0.len() + 1);
        let mut carry = bit as u32;
        for &d in &self.0 {
            out.push((d << 1) | carry);
            carry = d >> 31;
        }
        out.push(carry);
        let mut b = Big(out);
        b.trim();
        b
    }

    fn cmp(&self, o: &Big) -> Ordering {
        if self.0.len() != o.0.len() {
            return self.0.len().cmp(&o.0.len());
        }
        for i in (0..self.0.len()).rev() {
            if self.0[i] != o.0[i] {
                return self.0[i].cmp(&o.0[i]);
            }
        }
        Ordering::Equal
    }

    /// self / d as a float, rounded to nearest.
    fn ratio(&self, d: &Big) -> f64 {
        if self.is_zero() {
            return 0.0;
        }
        let shift = (d.bits() + 70).saturating_sub(self.bits());
        let total = self.bits() + shift;
        let mut q = Big(Vec::new());
        let mut r = Big(Vec::new());
        for i in (0..total).rev() {
            let bit = i >= shift && self.bit(i - shift);
            r = r.shl1_or(bit);
            let take = r.cmp(d) != Ordering::Less;
            if take {
                r = r.sub(d);
            }
            q = q.shl1_or(take);
        }
        let qb = q.bits();
        let drop = qb.saturating_sub(64);
        let mut top: u64 = 0;
        for i in (drop..qb).rev() {
            top = (top << 1) | q.bit(i) as u64;
        }
        let sticky = !r.is_zero() || (0..drop).any(|i| q.bit(i));
        if sticky {
            top |= 1;
        }
        (top as f64) * 2f64.powi(drop as i32 - shift as i32)
    }
}

fn choose(n: i64, k: i64) -> Big {
    if k < 0 || k > n {
        return Big(Vec::new());
    }
    let mut acc = Big::from(1);
    for i in 1..=k {
        acc = acc.mul_small((n - k + i) as u32).div_small(i as u32);
    }
    acc
}

/// Fisher's exact test, two-sided, on the table [[a, b], [c, d]].
pub fn fisher(a: i64, b: i64, c: i64, d: i64) -> f64 {
    let n1 = a + b;
    let n2 = c + d;
    let k = a + c;
    let total = choose(n1 + n2, k);
    let weight = |x: i64| choose(n1, x).mul(&choose(n2, k - x));
    let observed = weight(a);
    let mut s = Big(Vec::new());
    for x in (k - n2).max(0)..=k.min(n1) {
        let w = weight(x);
        if w.cmp(&observed) != Ordering::Greater {
            s = s.add(&w);
        }
    }
    s.ratio(&total)
}

fn choose_f(n: i64, k: i64) -> f64 {
    let c = choose(n, k);
    c.ratio(&Big::from(1))
}

/// The chance that Fisher's test gives p < 0.1 for na runs against nb when
/// both pass at the pooled rate of pa + pb passes.
pub fn chance_of_mark(pa: i64, na: i64, pb: i64, nb: i64) -> f64 {
    let rate = (pa + pb) as f64 / (na + nb) as f64;
    let binom =
        |n: i64, k: i64| choose_f(n, k) * rate.powf(k as f64) * (1.0 - rate).powf((n - k) as f64);
    (0..=na)
        .flat_map(|a| (0..=nb).map(move |b| (a, b)))
        .filter(|&(a, b)| fisher(a, na - a, b, nb - b) < 0.1)
        .map(|(a, b)| binom(na, a) * binom(nb, b))
        .sum()
}

// Runs.

/// A trial: its task, model and number.
type TrialKey = (Value, Value, Value);

/// A diagnostic's kind and template.
type Template = (String, String);

fn is_graded(r: &Value) -> bool {
    !matches!(
        r.get("outcome").as_str(),
        Some("infrastructure" | "isolation")
    )
}

pub fn counted(results: &Value) -> Vec<&Value> {
    results
        .get("runs")
        .items()
        .iter()
        .filter(|r| r.get("counts").truthy())
        .collect()
}

/// The final run of each trial: its graded attempt, or its last.
pub fn trials(results: &Value) -> Vec<&Value> {
    let mut groups: Vec<(TrialKey, Vec<&Value>)> = Vec::new();
    for r in counted(results) {
        let key = (
            r.get("task").clone(),
            r.get("model").clone(),
            r.get("trial").clone(),
        );
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some(g) => g.1.push(r),
            None => groups.push((key, vec![r])),
        }
    }
    groups
        .into_iter()
        .map(|(_, rs)| {
            if let Some(r) = rs.iter().find(|r| is_graded(r)) {
                return *r;
            }
            let mut best = rs[0];
            for r in &rs[1..] {
                if r.get("attempt").as_f64() > best.get("attempt").as_f64() {
                    best = r;
                }
            }
            best
        })
        .collect()
}

/// The graded final runs of each task and model.
pub struct Cells<'a>(Vec<((Value, Value), Vec<&'a Value>)>);

impl<'a> Cells<'a> {
    pub fn of(results: &'a Value) -> Cells<'a> {
        let mut out: Vec<((Value, Value), Vec<&Value>)> = Vec::new();
        for r in trials(results) {
            if !is_graded(r) {
                continue;
            }
            let key = (r.get("task").clone(), r.get("model").clone());
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some(c) => c.1.push(r),
                None => out.push((key, vec![r])),
            }
        }
        Cells(out)
    }

    pub fn get(&self, t: &Value, m: &Value) -> &[&'a Value] {
        self.0
            .iter()
            .find(|((a, b), _)| a == t && b == m)
            .map(|(_, v)| v.as_slice())
            .unwrap_or(&[])
    }
}

fn passes(runs: &[&Value]) -> usize {
    runs.iter()
        .filter(|r| r.get("outcome").as_str() == Some("pass"))
        .count()
}

pub fn pass_rate(runs: &[&Value]) -> Option<f64> {
    if runs.is_empty() {
        None
    } else {
        Some(passes(runs) as f64 / runs.len() as f64)
    }
}

pub fn task_names(results: &Value) -> Vec<Value> {
    let names = results.get("task_names");
    if names.truthy() {
        names.items().to_vec()
    } else {
        results
            .get("tasks")
            .members()
            .iter()
            .map(|(k, _)| Value::Str(k.clone()))
            .collect()
    }
}

// The report.

/// A float with digits after the point, or `–` for none.
pub fn fmt(x: Option<f64>, digits: usize) -> String {
    match x {
        None => "–".to_string(),
        Some(x) => format!("{x:.digits$}"),
    }
}

/// text as a Markdown code span; one with backquotes in it gets two.
pub fn code(text: &str) -> String {
    if text.contains('`') {
        format!("`` {text} ``")
    } else {
        format!("`{text}`")
    }
}

pub fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = format!(
        "| {} |\n| {} |\n",
        header.join(" | "),
        header.iter().map(|_| "---").collect::<Vec<_>>().join(" | ")
    );
    for r in rows {
        let cells: Vec<String> = r.iter().map(|c| c.replace('|', "\\|")).collect();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out
}

pub fn models_reported(results: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in results.get("runs").items() {
        for m in r.get("models_reported").items() {
            let m = m.to_text();
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    out.sort();
    out
}

fn join_text(items: &[Value], sep: &str) -> String {
    items
        .iter()
        .map(Value::to_text)
        .collect::<Vec<_>>()
        .join(sep)
}

fn effort(results: &Value) -> String {
    let e = results.get("effort");
    if e.truthy() {
        e.to_text()
    } else {
        "default".to_string()
    }
}

fn label_part(results: &Value) -> String {
    let l = results.get("label");
    if l.truthy() {
        format!(" ({})", l.to_text())
    } else {
        String::new()
    }
}

fn commit7(results: &Value) -> String {
    crate::text::first_chars(&results.get("commit").to_text(), 7).to_string()
}

fn run_name(r: &Value) -> String {
    format!(
        "{}/{}/{}.{}",
        r.get("task").to_text(),
        r.get("model").to_text(),
        r.get("trial").to_text(),
        r.get("attempt").to_text()
    )
}

/// Values of runs for a mean: none for null and false.
fn present<'a>(rs: &[&'a Value], f: impl Fn(&'a Value) -> Value) -> Vec<Value> {
    rs.iter().map(|r| f(r)).filter(Value::truthy).collect()
}

fn avg(values: &[Value]) -> Option<f64> {
    let refs: Vec<&Value> = values.iter().collect();
    mean(&refs)
}

pub fn render(results: &Value) -> String {
    let mut out = String::new();
    let cells = Cells::of(results);
    let tasks = task_names(results);
    let models = results.get("models").items();
    let trials_wanted = results.get("trials").as_i64().unwrap_or(0);
    out.push_str(&format!(
        "# Eval results {}, {}{}\n\n",
        results.get("date").to_text(),
        commit7(results),
        label_part(results)
    ));
    out.push_str(&format!(
        "- Commit: `{}`, Midnight {}\n",
        results.get("commit").to_text(),
        results.get("version").to_text()
    ));
    out.push_str(&format!(
        "- Claude Code {}\n",
        results.get("claude_code").to_text()
    ));
    out.push_str(&format!(
        "- Kit: `{}` ({} files)\n",
        results.get("kit").get("file").to_text(),
        results.get("kit").get("files").items().len()
    ));
    if results.get("agent_tools").truthy() {
        out.push_str(&format!(
            "- Agent's tools: `{}`; compiler's PATH: `{}`\n",
            results.get("agent_tools").to_text(),
            results.get("coreutils").to_text()
        ));
    }
    out.push_str(&format!(
        "- Models: {} (as reported: {}); effort: {}; trials: {}\n\n",
        join_text(models, ", "),
        models_reported(results).join(", "),
        effort(results),
        results.get("trials").to_text()
    ));

    out.push_str("## Pass rates\n\n");
    let rows: Vec<Vec<String>> = models
        .iter()
        .map(|m| {
            let rates: Vec<f64> = tasks
                .iter()
                .filter_map(|t| pass_rate(cells.get(t, m)))
                .collect();
            let every = tasks
                .iter()
                .filter(|t| {
                    let rs = cells.get(t, m);
                    !rs.is_empty() && passes(rs) == rs.len()
                })
                .count();
            let se = se(&rates);
            let pm = match se {
                Some(s) => format!(" ± {}", fmt(Some(s * 100.0), 1)),
                None => String::new(),
            };
            vec![
                format!("`{}`", m.to_text()),
                format!("{}%{pm}", fmt(fmean(&rates).map(|x| x * 100.0), 1)),
                format!("{every} of {}", tasks.len()),
            ]
        })
        .collect();
    out.push_str(&table(
        &[
            "Model",
            "pass@1 (± s.e. across tasks)",
            "Tasks passed every time",
        ],
        &rows,
    ));
    let mut short = Vec::new();
    for t in &tasks {
        for m in models {
            let n = cells.get(t, m).len();
            if (n as i64) < trials_wanted {
                short.push(format!("{}/{} ({n})", t.to_text(), m.to_text()));
            }
        }
    }
    if !short.is_empty() {
        out.push_str(&format!(
            "\nFewer graded runs than trials: {}.\n",
            short.join(", ")
        ));
    }

    out.push_str("\n## Tasks\n\n");
    let mut rows = Vec::new();
    for t in &tasks {
        for m in models {
            let rs = cells.get(t, m);
            let p = passes(rs);
            let mark = if (rs.len() as i64) < trials_wanted {
                " *"
            } else {
                ""
            };
            let a = |f: &dyn Fn(&Value) -> Value| avg(&present(rs, f));
            rows.push(vec![
                format!("`{}`", t.to_text()),
                format!("`{}`", m.to_text()),
                format!("{p}/{}{mark}", rs.len()),
                if !rs.is_empty() && p == rs.len() {
                    "yes"
                } else {
                    "no"
                }
                .to_string(),
                fmt(a(&|r| r.get("calls").get("total").clone()), 1),
                fmt(a(&|r| r.get("calls").get("failed_total").clone()), 1),
                fmt(a(&|r| r.get("turns").clone()), 1),
                fmt(
                    a(&|r| match r.get("duration_ms").as_f64() {
                        Some(d) if r.get("duration_ms").truthy() => Value::Float(d / 1000.0),
                        _ => Value::Null,
                    }),
                    0,
                ),
                fmt(a(&|r| r.get("output_tokens").clone()), 0),
                fmt(
                    a(&|r| {
                        Value::Int(
                            r.get("input_tokens").to_int()
                                + r.get("cache_read_tokens").to_int()
                                + r.get("cache_creation_tokens").to_int(),
                        )
                    }),
                    0,
                ),
                fmt(a(&|r| r.get("cost_usd").clone()), 3),
                rs.iter()
                    .filter(|r| r.get("first_try").truthy())
                    .count()
                    .to_string(),
                rs.iter()
                    .filter(|r| r.get("verified").truthy())
                    .count()
                    .to_string(),
            ]);
        }
    }
    out.push_str(&table(
        &[
            "Task",
            "Model",
            "Passed",
            "Every time",
            "Calls",
            "Failed calls",
            "Turns",
            "Seconds",
            "Output tokens",
            "Input tokens",
            "Cost (est. $)",
            "First try",
            "Verified",
        ],
        &rows,
    ));
    out.push_str("\nMeans over graded runs. Cost is Claude Code's estimate at API prices.\n");

    let graded: Vec<&Value> = trials(results)
        .into_iter()
        .filter(|r| is_graded(r))
        .collect();
    if graded.iter().any(|r| {
        r.get("checks")
            .as_obj()
            .is_some_and(|c| c.contains("draws"))
    }) {
        out.push_str(&draws_section(&tasks, models, &cells));
    }

    out.push_str("\n## Diagnostics and run-time errors\n\n");
    let mut groups: Vec<(Template, Vec<(usize, &Value)>)> = Vec::new();
    for (ri, r) in graded.iter().enumerate() {
        for d in r.get("diagnostics").items() {
            let key = (d.get("kind").to_text(), d.get("template").to_text());
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some(g) => g.1.push((ri, d)),
                None => groups.push((key, vec![(ri, d)])),
            }
        }
    }
    if groups.is_empty() {
        out.push_str("None.\n");
    } else {
        groups.sort_by(|(ka, va), (kb, vb)| vb.len().cmp(&va.len()).then_with(|| ka.cmp(kb)));
        let rows: Vec<Vec<String>> = groups
            .iter()
            .map(|((kind, tmpl), v)| {
                let mut runs: Vec<usize> = Vec::new();
                for (ri, _) in v {
                    if !runs.contains(ri) {
                        runs.push(*ri);
                    }
                }
                let mut tally: Vec<(Vec<String>, usize)> = Vec::new();
                for (_, d) in v {
                    let s: Vec<String> =
                        d.get("spans").items().iter().map(Value::to_text).collect();
                    if s.is_empty() {
                        continue;
                    }
                    match tally.iter_mut().find(|(k, _)| *k == s) {
                        Some(e) => e.1 += 1,
                        None => tally.push((s, 1)),
                    }
                }
                tally.sort_by(|(sa, ca), (sb, cb)| cb.cmp(ca).then_with(|| sa.cmp(sb)));
                let passed = runs
                    .iter()
                    .filter(|&&ri| graded[ri].get("outcome").as_str() == Some("pass"))
                    .count();
                vec![
                    kind.clone(),
                    code(tmpl),
                    v.len().to_string(),
                    runs.len().to_string(),
                    passed.to_string(),
                    v.iter()
                        .filter(|(_, d)| d.get("repeated").truthy())
                        .count()
                        .to_string(),
                    tally
                        .iter()
                        .take(6)
                        .map(|(s, c)| format!("{} ×{c}", s.join(" / ")))
                        .collect::<Vec<_>>()
                        .join(", "),
                ]
            })
            .collect();
        out.push_str(&table(
            &[
                "Kind", "Template", "Count", "Runs", "Passed", "Repeated", "Spans",
            ],
            &rows,
        ));
    }

    out.push_str("\n## The reference as read\n\n");
    let read_runs: Vec<&Value> = graded
        .iter()
        .copied()
        .filter(|r| r.get("read").truthy())
        .collect();
    if read_runs.is_empty() {
        out.push_str("Not recorded.\n");
    } else {
        let mut rows = Vec::new();
        for (dest, secs) in results.get("kit").get("sections").members() {
            let secs = secs.items();
            for (i, sec) in secs.iter().enumerate() {
                let start = sec.items().first().and_then(Value::as_f64).unwrap_or(0.0);
                let title = sec.items().get(1).filter(|t| t.truthy());
                let first = if title.is_some() { start + 1.0 } else { start };
                let stop = match secs.get(i + 1) {
                    Some(next) => next.items().first().and_then(Value::as_f64).unwrap_or(0.0) - 1.0,
                    None => f64::INFINITY,
                };
                let n = read_runs
                    .iter()
                    .filter(|r| {
                        r.get("read").get(dest).items().iter().any(|range| {
                            let a = range.items().first().and_then(Value::as_f64).unwrap_or(0.0);
                            let b = range.items().get(1).and_then(Value::as_f64).unwrap_or(0.0);
                            b >= first && a <= stop
                        })
                    })
                    .count();
                rows.push(vec![
                    format!("`{dest}`"),
                    title
                        .map(Value::to_text)
                        .unwrap_or_else(|| "(whole file)".to_string()),
                    n.to_string(),
                ]);
            }
        }
        out.push_str(&table(&["File", "Section", "Runs"], &rows));
        out.push_str(&format!(
            "\nRuns of {} graded that a tool result showed a line of the section to, below its heading.\n",
            read_runs.len()
        ));
    }

    out.push_str("\n## Exceptions\n\n");
    let all = counted(results);
    let pick = |f: &dyn Fn(&Value) -> bool| -> Vec<&Value> {
        graded.iter().copied().filter(|r| f(r)).collect()
    };
    let lists: Vec<(&str, Vec<&Value>)> = vec![
        ("Compiler bugs", pick(&|r| r.get("compiler_bug").truthy())),
        (
            "Escaped runs",
            pick(&|r| r.get("outcome").as_str() == Some("escaped")),
        ),
        (
            "Runs for review (calls the audit could not follow, calls the wrapper refused)",
            pick(&|r| r.get("review").truthy()),
        ),
        (
            "Infrastructure errors",
            results
                .get("runs")
                .items()
                .iter()
                .filter(|r| r.get("outcome").as_str() == Some("infrastructure"))
                .collect(),
        ),
        (
            "Timeouts",
            pick(&|r| r.get("outcome").as_str() == Some("timeout")),
        ),
        (
            "No solution",
            pick(&|r| r.get("outcome").as_str() == Some("no solution")),
        ),
        (
            "Calls stopped by the time limit under load",
            pick(&|r| r.get("slow_calls").to_int() > 0),
        ),
        (
            "Programs the time limit stopped in grading, graded again alone",
            pick(&|r| r.get("regraded").truthy()),
        ),
    ];
    for (name, rs) in &lists {
        if rs.is_empty() {
            continue;
        }
        let items: Vec<String> = rs
            .iter()
            .map(|r| format!("{}{}", run_name(r), detail(r)))
            .collect();
        out.push_str(&format!("- {name}: {}\n", items.join("; ")));
    }
    let mut denied: Vec<(usize, String)> = Vec::new();
    for (ri, r) in graded.iter().enumerate() {
        for d in r.get("denied").items() {
            let input = d.get("input").to_text();
            let first = crate::text::lines(&input)
                .first()
                .map(|l| crate::text::chomp(l).to_string())
                .unwrap_or_default();
            denied.push((
                ri,
                format!(
                    "{} `{}`",
                    d.get("tool").to_text(),
                    crate::text::first_chars(&first, 60)
                ),
            ));
        }
    }
    if !denied.is_empty() {
        let mut distinct = Vec::new();
        for (r, _) in &denied {
            if !distinct.contains(r) {
                distinct.push(*r);
            }
        }
        let mut tally: Vec<(String, usize)> = Vec::new();
        for (_, s) in &denied {
            match tally.iter_mut().find(|(k, _)| k == s) {
                Some(e) => e.1 += 1,
                None => tally.push((s.clone(), 1)),
            }
        }
        let shown: Vec<String> = tally
            .iter()
            .map(|(s, c)| {
                if *c > 1 {
                    format!("{s} ×{c}")
                } else {
                    s.clone()
                }
            })
            .collect();
        out.push_str(&format!(
            "- Denied tool calls: {} in {} runs: {}\n",
            denied.len(),
            distinct.len(),
            shown.join(", ")
        ));
    }
    if lists.iter().all(|(_, rs)| rs.is_empty()) && denied.is_empty() {
        out.push_str("None.\n");
    }
    out.push_str(&format!(
        "\n{} counted runs, {} graded.\n",
        all.len(),
        graded.len()
    ));
    out
}

/// The section on the check `draws`: for each task and model that has it,
/// the graded runs that built and failed it, by how their answers first
/// differ; then the runs whose first differing shape is the expected one
/// with two numbers exchanged, and those whose first differing shape is
/// the expected rectangle placed by its center.
fn draws_section(tasks: &[Value], models: &[Value], cells: &Cells) -> String {
    let mut out = String::from("\n## Draws\n\n");
    let hows = [
        "answers", "frame", "missing", "extra", "order", "differs", "cut",
    ];
    let mut swapped = Vec::new();
    let mut centered = Vec::new();
    let mut rows = Vec::new();
    for t in tasks {
        for m in models {
            let rs: Vec<&Value> = cells
                .get(t, m)
                .iter()
                .copied()
                .filter(|r| {
                    r.get("checks")
                        .as_obj()
                        .is_some_and(|c| c.contains("draws"))
                })
                .collect();
            if rs.is_empty() {
                continue;
            }
            let failed: Vec<&Value> = rs
                .iter()
                .copied()
                .filter(|r| {
                    r.get("checks").get("build").truthy()
                        && r.get("checks").get("draws") == &Value::Bool(false)
                })
                .collect();
            for r in &failed {
                let name = format!(
                    "{}/{}/{}.{}",
                    t.to_text(),
                    m.to_text(),
                    r.get("trial").to_text(),
                    r.get("attempt").to_text()
                );
                let d = r.get("draws");
                if d.get("how").as_str() != Some("differs") {
                    continue;
                }
                let (got, want) = (d.get("got").as_str(), d.get("want").as_str());
                if draws::swapped(got, want) {
                    swapped.push(name.clone());
                }
                if draws::by_center(got, want) {
                    centered.push(name);
                }
            }
            let mut row = vec![
                format!("`{}`", t.to_text()),
                format!("`{}`", m.to_text()),
                rs.len().to_string(),
                failed.len().to_string(),
            ];
            for h in hows {
                let n = failed
                    .iter()
                    .filter(|r| r.get("draws").get("how").as_str() == Some(h))
                    .count();
                row.push(n.to_string());
            }
            rows.push(row);
        }
    }
    let capitalized: Vec<String> = hows
        .iter()
        .map(|h| {
            let mut c = h.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect();
    let mut header = vec!["Task", "Model", "Graded", "Built and failed draws"];
    header.extend(capitalized.iter().map(String::as_str));
    out.push_str(&table(&header, &rows));
    out.push_str("\nHow each run's answers first differ from the expected ones.\n\n");
    let list = |v: &Vec<String>| {
        if v.is_empty() {
            "none".to_string()
        } else {
            v.join(", ")
        }
    };
    out.push_str(&format!("- Two numbers exchanged: {}\n", list(&swapped)));
    out.push_str(&format!(
        "- A rectangle placed by its center: {}\n",
        list(&centered)
    ));
    out
}

fn detail(r: &Value) -> String {
    match r.get("outcome").as_str() {
        Some("infrastructure") => format!(" ({})", r.get("infrastructure").to_text()),
        Some("escaped") => {
            let paths: Vec<String> = r
                .get("escapes")
                .items()
                .iter()
                .map(|e| e.get("path").to_text())
                .collect();
            format!(" ({})", paths.join(", "))
        }
        _ => {
            let mut parts: Vec<String> = r
                .get("review")
                .items()
                .iter()
                .map(|e| e.get("what").to_text())
                .take(3)
                .collect();
            if r.get("regraded").truthy() {
                parts.push(format!(
                    "{} under load, {} alone",
                    r.get("regraded").get("before").to_text(),
                    r.get("outcome").to_text()
                ));
            }
            if parts.is_empty() {
                String::new()
            } else {
                format!(" ({})", parts.join("; "))
            }
        }
    }
}

// Comparison.

fn pct(x: Option<f64>, signed: bool) -> String {
    match x {
        None => "–".to_string(),
        Some(x) => {
            let s = format!("{:.1}", x * 100.0);
            if signed && !s.starts_with('-') {
                format!("+{s}%")
            } else {
                format!("{s}%")
            }
        }
    }
}

fn intersect(a: &[Value], b: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for x in a {
        if b.contains(x) && !out.contains(x) {
            out.push(x.clone());
        }
    }
    out
}

pub fn render_compare(a: &Value, b: &Value) -> String {
    let mut out = String::from("# Comparison\n\n");
    for (name, r) in [("A", a), ("B", b)] {
        out.push_str(&format!(
            "- {name}: {}, {}{}\n",
            r.get("date").to_text(),
            commit7(r),
            label_part(r)
        ));
    }
    out.push('\n');
    let diffs = differences(a, b);
    for d in &diffs {
        out.push_str(&format!("- {d}\n"));
    }
    if !diffs.is_empty() {
        out.push('\n');
    }
    let ca = Cells::of(a);
    let cb = Cells::of(b);
    let tasks = intersect(&task_names(a), &task_names(b));
    let models = intersect(a.get("models").items(), b.get("models").items());

    out.push_str("## Pass rates\n\n");
    let rows: Vec<Vec<String>> = models
        .iter()
        .map(|m| {
            let ra: Vec<f64> = tasks
                .iter()
                .filter_map(|t| pass_rate(ca.get(t, m)))
                .collect();
            let rb: Vec<f64> = tasks
                .iter()
                .filter_map(|t| pass_rate(cb.get(t, m)))
                .collect();
            let diffs: Vec<f64> = tasks
                .iter()
                .filter_map(|t| {
                    let x = pass_rate(ca.get(t, m))?;
                    let y = pass_rate(cb.get(t, m))?;
                    Some(y - x)
                })
                .collect();
            let pm = match se(&diffs) {
                Some(s) => format!(" ± {}", fmt(Some(s * 100.0), 1)),
                None => String::new(),
            };
            vec![
                format!("`{}`", m.to_text()),
                pct(fmean(&ra), false),
                pct(fmean(&rb), false),
                format!("{}{pm}", pct(fmean(&diffs), true)),
                if ra.len() == rb.len() {
                    String::new()
                } else {
                    format!("{}/{}", ra.len(), rb.len())
                },
            ]
        })
        .collect();
    out.push_str(&table(
        &["Model", "pass@1 A", "pass@1 B", "Difference (± s.e.)", ""],
        &rows,
    ));
    out.push_str("\nOver the tasks in both.\n");

    out.push_str("\n## Tasks\n\n");
    let mut marks = 0;
    let mut chance = 0.0;
    let mut pooled = [0i64; 4];
    let mut rows = Vec::new();
    for t in &tasks {
        for m in &models {
            let x = ca.get(t, m);
            let y = cb.get(t, m);
            let pa = passes(x) as i64;
            let pb = passes(y) as i64;
            let (nx, ny) = (x.len() as i64, y.len() as i64);
            pooled[0] += pa;
            pooled[1] += nx;
            pooled[2] += pb;
            pooled[3] += ny;
            let p = if x.is_empty() || y.is_empty() {
                None
            } else {
                Some(fisher(pa, nx - pa, pb, ny - pb))
            };
            if p.is_some() {
                chance += chance_of_mark(pa, nx, pb, ny);
            }
            let mark = p.is_some_and(|p| p < 0.1);
            if mark {
                marks += 1;
            }
            rows.push(vec![
                format!("`{}`", t.to_text()),
                format!("`{}`", m.to_text()),
                format!("{pa}/{nx}"),
                format!("{pb}/{ny}"),
                p.map(|p| format!("{p:.3}"))
                    .unwrap_or_else(|| "–".to_string()),
                if mark { "**differs**" } else { "" }.to_string(),
            ]);
        }
    }
    out.push_str(&table(&["Task", "Model", "A", "B", "Fisher p", ""], &rows));
    out.push_str(&format!(
        "\n{marks} marked (p < 0.1). Chance alone, with each row passing at its pooled rate in both, would mark about {} of {} rows.\n",
        fmt(Some(chance), 1),
        rows.len()
    ));
    out.push_str(&format!(
        "\n{}\n",
        pooled_line(pooled[0], pooled[1], pooled[2], pooled[3])
    ));

    out.push_str("\n## Templates whose count changed\n\n");
    let ta = template_counts(a, &tasks);
    let tb = template_counts(b, &tasks);
    let count = |m: &Vec<((String, String), i64)>, k: &(String, String)| {
        m.iter().find(|(x, _)| x == k).map(|(_, n)| *n).unwrap_or(0)
    };
    let mut keys: Vec<(String, String)> = Vec::new();
    for (k, _) in ta.iter().chain(tb.iter()) {
        if !keys.contains(k) {
            keys.push(k.clone());
        }
    }
    let mut changed: Vec<(String, String)> = keys
        .into_iter()
        .filter(|k| count(&ta, k) != count(&tb, k))
        .collect();
    changed.sort_by(|x, y| {
        let dx = (count(&tb, x) - count(&ta, x)).abs();
        let dy = (count(&tb, y) - count(&ta, y)).abs();
        dy.cmp(&dx).then_with(|| x.cmp(y))
    });
    if changed.is_empty() {
        out.push_str("None.\n");
    } else {
        let rows: Vec<Vec<String>> = changed
            .iter()
            .map(|k| {
                vec![
                    k.0.clone(),
                    code(&k.1),
                    count(&ta, k).to_string(),
                    count(&tb, k).to_string(),
                ]
            })
            .collect();
        out.push_str(&table(&["Kind", "Template", "A", "B"], &rows));
        out.push_str("\nOccurrences over the tasks in both, in all their runs.\n");
    }
    out
}

/// The passes and graded runs of A and of B over all the rows, and
/// Fisher's exact test, two-sided, on their passes and failures.
pub fn pooled_line(pa: i64, na: i64, pb: i64, nb: i64) -> String {
    let p = if na == 0 || nb == 0 {
        None
    } else {
        Some(fisher(pa, na - pa, pb, nb - pb))
    };
    let rate = |p: i64, n: i64| (n != 0).then(|| p as f64 / n as f64);
    format!(
        "Over all rows: A passed {pa} of {na} graded runs ({}), B {pb} of {nb} ({}); Fisher's exact test, two-sided: p = {}.",
        pct(rate(pa, na), false),
        pct(rate(pb, nb), false),
        p.map(|p| format!("{p:.3}"))
            .unwrap_or_else(|| "–".to_string())
    )
}

fn template_counts(results: &Value, tasks: &[Value]) -> Vec<((String, String), i64)> {
    let mut counts: Vec<((String, String), i64)> = Vec::new();
    for r in trials(results) {
        if !tasks.contains(r.get("task")) {
            continue;
        }
        for d in r.get("diagnostics").items() {
            let key = (d.get("kind").to_text(), d.get("template").to_text());
            match counts.iter_mut().find(|(k, _)| *k == key) {
                Some(e) => e.1 += 1,
                None => counts.push((key, 1)),
            }
        }
    }
    counts
}

fn keys(v: &Value) -> Vec<String> {
    v.members().iter().map(|(k, _)| k.clone()).collect()
}

/// What differs between two runs besides the commit.
fn differences(a: &Value, b: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let (ka, kb) = (keys(a.get("tasks")), keys(b.get("tasks")));
    let changed: Vec<String> = ka
        .iter()
        .filter(|t| kb.contains(t))
        .filter(|t| a.get("tasks").get(t).get("sha256") != b.get("tasks").get(t).get("sha256"))
        .map(|t| format!("`{t}`"))
        .collect();
    if !changed.is_empty() {
        out.push(format!("Tasks that differ: {}", changed.join(", ")));
    }
    let only_a: Vec<&String> = ka.iter().filter(|t| !kb.contains(t)).collect();
    let only_b: Vec<&String> = kb.iter().filter(|t| !ka.contains(t)).collect();
    let join = |v: &[&String]| v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
    if !only_a.is_empty() {
        out.push(format!("Tasks only in A: {}", join(&only_a)));
    }
    if !only_b.is_empty() {
        out.push(format!("Tasks only in B: {}", join(&only_b)));
    }
    let (ma, mb) = (models_reported(a), models_reported(b));
    if ma != mb {
        out.push(format!(
            "Models as reported: A {}; B {}",
            ma.join(", "),
            mb.join(", ")
        ));
    }
    if a.get("effort") != b.get("effort") {
        out.push(format!("Effort: A {}; B {}", effort(a), effort(b)));
    }
    if a.get("trials") != b.get("trials") {
        out.push(format!(
            "Trials: A {}; B {}",
            a.get("trials").to_text(),
            b.get("trials").to_text()
        ));
    }
    if a.get("claude_code") != b.get("claude_code") {
        out.push(format!(
            "Claude Code: A {}; B {}",
            a.get("claude_code").to_text(),
            b.get("claude_code").to_text()
        ));
    }
    if a.get("preamble_sha256") != b.get("preamble_sha256") {
        out.push("The preamble differs".to_string());
    }
    if a.get("agent_tools") != b.get("agent_tools") || a.get("coreutils") != b.get("coreutils") {
        out.push("The agent's tools or the compiler's PATH differ".to_string());
    }
    if let Some(k) = kit_difference(a.get("kit"), b.get("kit")) {
        out.push(k);
    }
    out
}

fn kit_files(kit: &Value) -> Vec<(String, &Value)> {
    let mut out: Vec<(String, &Value)> = Vec::new();
    for f in kit.get("files").items() {
        let d = f.get("dest").to_text();
        match out.iter_mut().find(|(k, _)| *k == d) {
            Some(e) => e.1 = f,
            None => out.push((d, f)),
        }
    }
    out
}

fn kit_difference(ka: &Value, kb: &Value) -> Option<String> {
    let fa = kit_files(ka);
    let fb = kit_files(kb);
    fn has<'a>(v: &[(String, &'a Value)], d: &str) -> Option<&'a Value> {
        v.iter().find(|(k, _)| k == d).map(|(_, f)| *f)
    }
    let mut parts = Vec::new();
    let only_a: Vec<&str> = fa
        .iter()
        .filter(|(d, _)| has(&fb, d).is_none())
        .map(|(d, _)| d.as_str())
        .collect();
    let only_b: Vec<&str> = fb
        .iter()
        .filter(|(d, _)| has(&fa, d).is_none())
        .map(|(d, _)| d.as_str())
        .collect();
    if !only_a.is_empty() {
        parts.push(format!("only in A: {}", only_a.join(", ")));
    }
    if !only_b.is_empty() {
        parts.push(format!("only in B: {}", only_b.join(", ")));
    }
    let changed: Vec<String> = fa
        .iter()
        .filter_map(|(d, f)| {
            let g = has(&fb, d)?;
            (f.get("sha256") != g.get("sha256")).then(|| {
                format!(
                    "{d} (A `{}`, B `{}`)",
                    f.get("source").to_text(),
                    g.get("source").to_text()
                )
            })
        })
        .collect();
    if !changed.is_empty() {
        parts.push(format!("changed: {}", changed.join(", ")));
    }
    if parts.is_empty() && ka.get("file") == kb.get("file") {
        return None;
    }
    let tail = if parts.is_empty() {
        String::new()
    } else {
        format!("; {}", parts.join("; "))
    };
    Some(format!(
        "Kit: A `{}`, B `{}`{tail}",
        ka.get("file").to_text(),
        kb.get("file").to_text()
    ))
}
