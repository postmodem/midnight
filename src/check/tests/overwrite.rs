//! Which terms the checker marks overwritten.

use super::*;
use crate::tir::Term;

/// For every `each` of the program, in the order functions, systems and the
/// top level are visited, its component terms that are overwritten, by
/// name.
fn overwritten(text: &str) -> Vec<Vec<String>> {
    let mut program = ok(text);
    let names: Vec<String> = program.components.iter().map(|c| c.name.clone()).collect();
    let mut found = Vec::new();
    let mut visit = |block: &mut tir::Block| {
        walk::block(block, &mut |node| {
            if let walk::Node::Stmt(Stmt {
                kind: StmtKind::Each { query, .. },
                ..
            }) = node
            {
                found.push(
                    query
                        .terms
                        .iter()
                        .filter_map(|term| match term {
                            Term::Component {
                                component,
                                overwritten: true,
                                ..
                            } => Some(names[component.0].clone()),
                            _ => None,
                        })
                        .collect(),
                );
            }
        });
    };
    for func in &mut program.funcs {
        visit(&mut func.body.block);
    }
    for system in &mut program.systems {
        visit(&mut system.body.block);
    }
    visit(&mut program.main.block);
    found
}

const DATA: &str = "\
component P
  x: Int32
  y: Int32
end

component Q
  a: Int32
  b: Int32
end

component W
  v: Vec
  n: Int32
end

struct Vec
  x: Int32
  y: Int32
end
";

fn program(body: &str) -> String {
    format!("{DATA}\n{body}")
}

fn none() -> Vec<String> {
    Vec::new()
}

fn just(name: &str) -> Vec<String> {
    vec![name.to_string()]
}

#[test]
fn a_whole_assignment_or_every_field_overwrites() {
    let found = overwritten(&program(
        "system s\n  each P, Q do |p, q|\n    k = 2\n    p = P(k, 0)\n    q.b = 1\n    q.a = k\n  end\nend\n",
    ));
    assert_eq!(found, vec![vec!["P".to_string(), "Q".to_string()]]);
}

#[test]
fn a_read_before_the_last_assignment_does_not() {
    let found = overwritten(&program(
        "system s\n  each P do |p|\n    p.x = 1\n    p.y = p.x\n  end\n  each P do |p|\n    k = p.y\n    p = P(k, k)\n  end\n  \
         each P do |p|\n    p.x = 1\n    p.y = 2\n    puts p.x\n  end\nend\n",
    ));
    assert_eq!(found, vec![none(), none(), just("P")]);
}

#[test]
fn a_compound_assignment_reads() {
    let found = overwritten(&program(
        "system s\n  each P do |p|\n    p.x += 1\n    p.y = 2\n    p.x = 3\n  end\nend\n",
    ));
    assert_eq!(found, vec![none()]);
}

#[test]
fn a_field_left_out_or_assigned_in_part_does_not() {
    let found = overwritten(&program(
        "system s\n  each P do |p|\n    p.x = 1\n  end\n  each W do |w|\n    w.v.x = 1\n    w.v.y = 2\n    w.n = 3\n  end\n  \
         each W do |w|\n    w.v = Vec(1, 2)\n    w.n = 3\n  end\nend\n",
    ));
    assert_eq!(found, vec![none(), none(), just("W")]);
}

#[test]
fn an_assignment_in_a_branch_or_after_one_does_not_count() {
    let found = overwritten(&program(
        "system s\n  each P do |p|\n    if p.x > 0\n      p = P(1, 1)\n    else\n      p = P(2, 2)\n    end\n  end\n  \
         each P do |p|\n    k = 1\n    k += 1 if k > 0\n    p = P(k, k)\n  end\nend\n",
    ));
    assert_eq!(found, vec![none(), none()]);
}

#[test]
fn leaving_early_does_not() {
    let found = overwritten(&program(
        "def f -> Int\n  each P do |p|\n    p = P(1, 1)\n    return 1 if p.x > 0\n  end\n  0\nend\n\n\
         system s\n  each P do |p|\n    p = P(1, 1)\n    break if p.x > 0\n  end\n  \
         each P do |p|\n    p = P(1, 1)\n    3.times do\n      break\n    end\n    next if p.y > 0\n  end\nend\n",
    ));
    assert_eq!(found, vec![none(), none(), just("P")]);
}

#[test]
fn another_way_to_the_component_does_not() {
    let found = overwritten(&program(
        "def peek(e: Entity) -> Int\n  e[P].x\nend\n\n\
         def peek_twice(e: Entity) -> Int\n  peek(e) + peek(e)\nend\n\n\
         def count_q -> Int\n  n = 0\n  each Q do |q|\n    n += q.a\n  end\n  n\nend\n\n\
         system s\n  each Entity, P do |e, p|\n    p = P(1, 1)\n    puts peek_twice(e)\n  end\n  \
         each Entity, P do |e, p|\n    p = P(1, 1)\n    puts e[P].y\n  end\n  \
         each P do |p|\n    p = P(1, 1)\n    each P do |other|\n      puts other.x\n    end\n  end\n  \
         each P do |p|\n    p = P(1, 1)\n    each P, Q do |other, q|\n      puts q.a\n    end\n  end\n  \
         each P, Q do |p, q|\n    p = P(1, 1)\n    q = Q(2, 2)\n    puts count_q\n  end\nend\n",
    ));
    // The functions' `each` comes first: count_q's, which assigns nothing.
    assert_eq!(
        found,
        vec![
            none(),
            none(),
            none(),
            none(),
            none(),
            just("P"),
            none(),
            just("P")
        ]
    );
}

#[test]
fn gpu_blocks_follow_the_same_run_and_their_nested_each() {
    let found = overwritten(&program(
        "gpu system g\n  each P, Q do |p, q|\n    q.a = p.x * 2\n    q.b = p.y\n  end\n  \
         each P do |p|\n    p.x = 1\n    p.y = p.x\n  end\n  \
         each P do |p|\n    p.x = 1\n    p.y = 2\n    s = 0\n    each P do |other|\n      s += other.x\n    end\n    p.x += s\n  end\n  \
         each P do |p|\n    p.x = 1\n    p.y = 2\n    s = 0\n    each Q do |other|\n      s += other.a\n    end\n    p.x += s\n  end\nend\n",
    ));
    assert_eq!(
        found,
        vec![just("Q"), none(), none(), none(), just("P"), none()]
    );
}

#[test]
fn a_term_the_body_does_not_name_is_not() {
    let found = overwritten(&program(
        "system s\n  each P, Q do |p, q|\n    p = P(1, 1)\n  end\nend\n",
    ));
    assert_eq!(found, vec![just("P")]);
}
