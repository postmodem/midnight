# frozen_string_literal: true

require "minitest/autorun"
require "json"
require "fileutils"
require_relative "../eval"

# The check `draws`: reading answers, comparing them by what they draw, what a
# failed check records, and the section of the report and line of compare
# that read it.
class DrawsTest < Minitest::Test
  FIXTURES = File.join(__dir__, "fixtures")
  D = Eval::Draws

  ANSWER = <<~TEXT
    draws: 3 ticks, 320x200, 3 draws
      Window.background = Color(16, 16, 32, 255)
      Draw.rect(10.0, 20.0, 30.0, 6.0, Color(40, 40, 60, 255))
      Draw.circle(26.0, 34.5, 4.0, Color(255, 200, 0, 255))
      Draw.line(NaN, -0.0, 1e+30, Infinity, 2.0, Color(1, 2, 3, 128))
  TEXT

  # ANSWER with its numbers written otherwise: the same draws.
  RESPELT = <<~TEXT
    draws: 3 ticks, 320x200, 3 draws
      Window.background = Color(16, 16, 32, 255)
      Draw.rect(10, 20.00, 30.0, 6.0, Color(40, 40, 60, 255))
      Draw.circle(26.0, 34.5, 4.0, Color(255, 200, 0, 255))
      Draw.line(NaN, -0.0, 1000000000000000019884624838656.0, Infinity, 2, Color(1, 2, 3, 128))
  TEXT

  def diff(got, want = ANSWER, any: false) = D.difference(D.parse(got), D.parse(want), any: any)

  def test_answers_are_taken_from_standard_error
    err = "midnight: drive: tick 3\n" + ANSWER.lines.map { |l| "midnight: drive: #{l}" }.join +
          "runtime error\nmidnight: drive:   not an answer's line\nmidnight: drive: draws: 4 ticks, 1x1, 0 draws\n" \
          "midnight: drive:   Window.background = Color(0, 0, 0, 0)\n"
    assert_equal "#{ANSWER}draws: 4 ticks, 1x1, 0 draws\n  Window.background = Color(0, 0, 0, 0)\n", D.answers_text(err)
  end

  def test_the_same_draws_written_otherwise
    a = D.parse(ANSWER).first
    assert_equal [3, "320x200", [16, 16, 32, 255]], a["frame"]
    assert_equal({ "kind" => "rect", "numbers" => [10.0, 20.0, 30.0, 6.0].map { |x| [x].pack("e").unpack1("V") },
                   "color" => [40, 40, 60, 255] }, a["shapes"].first)
    assert_equal [0x7fc00000, 0x80000000, D.bits("1e+30"), 0x7f800000, 0x40000000], a["shapes"].last["numbers"]
    assert_nil diff(RESPELT)
    assert_nil diff(ANSWER, RESPELT)
    assert_equal "text", D.shape("Draw.rect(1.0, 2.0, Color(0, 0, 0, 255))")["kind"]
    assert_equal "text", D.shape("Draw.oval(1.0, 2.0, 3.0, Color(0, 0, 0, 255))")["kind"]
  end

  def test_in_order
    lines = ANSWER.lines
    swapped_lines = lines.dup
    swapped_lines[2], swapped_lines[3] = swapped_lines[3], swapped_lines[2]
    assert_equal({ "how" => "order", "got" => lines[3].strip, "want" => lines[2].strip, "answer" => 1 },
                 diff(swapped_lines.join))
    assert_equal({ "how" => "missing", "got" => nil, "want" => lines[3].strip, "answer" => 1 },
                 diff((lines[0..2] + lines[4..]).join))
    extra = "  Draw.rect(0.0, 0.0, 0.0, 5.0, Color(1, 1, 1, 255))\n"
    assert_equal({ "how" => "extra", "got" => extra.strip, "want" => nil, "answer" => 1 }, diff(ANSWER + extra))
    assert_equal "differs", diff(ANSWER.sub("26.0, 34.5", "26.0, 34.0"))["how"]
    assert_equal "frame", diff(ANSWER.sub("3 ticks", "4 ticks"))["how"]
    assert_equal "frame", diff(ANSWER.sub("Color(16, 16, 32, 255)", "Color(16, 16, 32, 0)"))["how"]
    assert_nil diff(ANSWER.sub("3 draws", "5 draws")), "the count is the shapes'"
    two = ANSWER + ANSWER.sub("3 ticks", "6 ticks")
    assert_equal({ "answer" => 2, "how" => "answers", "got" => nil, "want" => "draws: 6 ticks, 320x200, 3 draws" },
                 diff(ANSWER, two))
    second = ANSWER + ANSWER.sub("3 ticks", "6 ticks").sub("Color(40, 40, 60, 255)", "Color(40, 60, 40, 255)")
    assert_equal({ "how" => "differs", "got" => "Draw.rect(10.0, 20.0, 30.0, 6.0, Color(40, 60, 40, 255))",
                   "want" => "Draw.rect(10.0, 20.0, 30.0, 6.0, Color(40, 40, 60, 255))", "answer" => 2 }, diff(second, two))
  end

  def test_any_order
    lines = ANSWER.lines
    reordered = [lines[0], lines[1], lines[4], lines[2], lines[3]].join
    assert_nil diff(reordered, any: true)
    refute_nil diff(reordered, any: false)
    twice = ANSWER + lines[2]
    assert_equal "extra", diff(twice, any: true)["how"]
    assert_equal "missing", diff(ANSWER, twice, any: true)["how"]
    near = ANSWER.sub("Draw.circle(26.0, 34.5", "Draw.circle(34.5, 26.0")
    d = diff(near, any: true)
    assert_equal({ "how" => "differs", "got" => "Draw.circle(34.5, 26.0, 4.0, Color(255, 200, 0, 255))",
                   "want" => lines[3].strip, "answer" => 1 }, d)
    assert D.swapped?(d["got"], d["want"])
    assert_equal "frame", diff(ANSWER.sub("320x200", "200x320"), any: true)["how"]
  end

  def test_swapped_and_by_centre
    want = "Draw.rect(10.0, 20.0, 30.0, 6.0, Color(40, 40, 60, 255))"
    assert D.swapped?("Draw.rect(20.0, 10.0, 30.0, 6.0, Color(40, 40, 60, 255))", want)
    assert D.swapped?("Draw.rect(10.0, 20.0, 6.0, 30.0, Color(40, 40, 60, 255))", want)
    refute D.swapped?("Draw.rect(20.0, 10.0, 6.0, 30.0, Color(40, 40, 60, 255))", want)
    refute D.swapped?("Draw.rect(20.0, 10.0, 30.0, 6.0, Color(40, 40, 61, 255))", want)
    refute D.swapped?(want, want)
    refute D.swapped?(nil, want)
    assert D.by_centre?("Draw.rect(25.0, 23.0, 30.0, 6.0, Color(0, 0, 0, 255))", want)
    refute D.by_centre?("Draw.rect(-5.0, 17.0, 30.0, 6.0, Color(40, 40, 60, 255))", want)
    refute D.by_centre?("Draw.circle(25.0, 23.0, 30.0, Color(40, 40, 60, 255))", want)
  end

  def test_a_draws_answer_is_one_record
    err = "midnight: drive: tick 3\n" + ANSWER.lines.map { |l| "midnight: drive: #{l}" }.join +
          "midnight: drive: try.drive:4: can't list the draws: no window is open\n"
    assert_equal ["drive: tick 3", "drive: draws", "drive: try.drive:4: can't list the draws: no window is open"],
                 Eval::Stderr.parse(err).map { |d| d["message"] }
  end

  # A task in a fresh directory, and a stub compiler whose `build` makes a
  # program that prints what solution.12am holds: its standard output
  # before a line `---`, its standard error after.
  def with_task(any: false)
    dir = Eval::Env.mktemp
    task = File.join(dir, "task")
    FileUtils.mkdir_p(task)
    File.write(File.join(task, "meta"), "areas: draw window\ncontrol: no\nruby: x\ndraws: #{any ? 'any' : 'in'} order\n")
    File.write(File.join(task, "expected.out"), "tick 3\n")
    File.write(File.join(task, "expected.draws"), ANSWER)
    File.write(File.join(task, "check.drive"), "tick 3\ndraws\n")
    stub = File.join(dir, "stub")
    File.write(stub, <<~RUBY)
      #!#{Eval::Env.ruby} --disable-gems
      exit 0 unless ARGV.first == "build"
      exit 1 if File.read("solution.12am").include?("FAIL")
      out, err = File.read("solution.12am").split("---\\n", 2)
      File.write("solution", "#!#{Eval::Env.ruby} --disable-gems\\n$stdout.write(\#{out.inspect})\\n$stderr.write(\#{err.to_s.inspect})\\n")
      File.chmod(0o755, "solution")
    RUBY
    File.chmod(0o755, stub)
    yield Eval::Task.new(task), stub, dir
  ensure
    FileUtils.rm_rf(dir)
  end

  def program(dir, err, out: "tick 3\n")
    path = File.join(dir, "prog.12am")
    File.write(path, "#{out}---\n#{err}")
    path
  end

  def said(text) = text.lines.map { |l| "midnight: drive: #{l}" }.join

  def test_the_check_grades_and_keeps_the_answers
    with_task do |task, stub, dir|
      keep = File.join(dir, "keep")
      g = Eval::Grade.grade(task, program(dir, said(RESPELT)), compiler: stub, keep: keep)
      assert_equal "pass", g["verdict"]
      assert_equal({ "build" => true, "output" => true, "draws" => true }, g["checks"])
      assert_equal RESPELT, File.read(File.join(keep, "graded.draws"))

      g = Eval::Grade.grade(task, program(dir, said(ANSWER.sub("26.0, 34.5", "34.5, 26.0"))), compiler: stub, keep: keep)
      assert_equal "fail", g["verdict"]
      assert_equal({ "build" => true, "output" => true, "draws" => false }, g["checks"])
      assert_equal "differs", g["draws"]["how"]
      assert_match(/answer 1: differs, got `Draw.circle\(34.5, 26.0/, g["notes"]["draws"])
      rec = {}
      Eval.apply_grade(rec, g, escaped: false, timeout: false)
      assert_equal ["draws"], rec["failed_checks"]
      assert_equal 1, rec["draws"]["answer"]

      g = Eval::Grade.grade(task, program(dir, "y" * (Eval::CAP + 10)), compiler: stub)
      assert_equal "cut", g["draws"]["how"]

      g = Eval::Grade.grade(task, program(dir, said(ANSWER), out: "tick 4\n"), compiler: stub)
      assert_equal({ "build" => true, "output" => false, "draws" => true }, g["checks"])
    end
  end

  # The thirteen tasks' hashes, as the baseline of spec 0008 recorded them
  # at a8f0a5f; a task changed since then is left out.
  def test_self_test_wants_a_wrong_program_that_fails_only_draws
    with_task do |task, stub, _|
      write = ->(rel, text) { FileUtils.mkdir_p(File.dirname(task.path(rel))); File.write(task.path(rel), text) }
      write.("TASK.md", "Draw.\n\n```\ntick 1\n```\n")
      write.("expected.out", "tick 1\ntick 3\n")
      good = "tick 1\ntick 3\n---\n#{said(ANSWER)}"
      write.("solution.12am", good)
      write.("right/respelt.12am", "tick 1\ntick 3\n---\n#{said(RESPELT)}")
      write.("wrong/build.12am", "FAIL\n")
      root = File.dirname(task.dir)
      _, problems = Eval::SelfTest.run(root, compiler: stub)
      assert_equal ["task: no program in wrong/ fails only `draws`"], problems
      write.("wrong/swapped.12am", good.sub("26.0, 34.5", "34.5, 26.0"))
      _, problems = Eval::SelfTest.run(root, compiler: stub)
      assert_empty problems
      write.("meta", task.read("meta").sub("draws: in order\n", ""))
      _, problems = Eval::SelfTest.run(root, compiler: stub)
      assert_equal ["task: expected.draws and `draws: in order` or `draws: any order` in meta go together"], problems
    end
  end

  def test_tasks_without_expected_draws_keep_their_hash
    baseline = File.join(Eval::ROOT, "evals", "results", "2026-11-26-a8f0a5f-baseline.json")
    if File.exist?(baseline) && thirteen_unchanged?
      JSON.parse(File.read(baseline))["tasks"].each do |name, t|
        assert_equal t["sha256"], Eval::Task.new(File.join(Eval::ROOT, "evals", "tasks", name)).sha256, name
      end
    end
    with_task do |task, _, _|
      before = task.sha256
      File.write(task.path("expected.draws"), ANSWER.sub("3 ticks", "4 ticks"))
      refute_equal before, task.sha256
    end
  end

  # Whether the thirteen tasks' files are as they were at the baseline's
  # commit (false without git).
  def thirteen_unchanged?
    names = %w[countdown crowd fixup league ledger mend orbits paddle pantry sandpile swarm thermostat warren]
    system("git", "-C", Eval::ROOT, "diff", "--quiet", "a8f0a5f", "--", *names.map { |n| "evals/tasks/#{n}" },
           out: File::NULL, err: File::NULL)
  end

  def results(name) = JSON.parse(File.read(File.join(FIXTURES, name)))

  def test_compare_pools_every_row
    text = Eval::Compare.render(results("results-a.json"), results("results-b.json"))
    assert_match(/Over all rows: A passed 6 of 6 graded runs \(100\.0%\), B 5 of 6 \(83\.3%\); Fisher's exact test, two-sided: p = 1\.000\./, text)
    assert_equal "Over all rows: A passed 3 of 10 graded runs (30.0%), B 9 of 10 (90.0%); Fisher's exact test, two-sided: p = 0.020.",
                 Eval::Compare.pooled_line(3, 10, 9, 10)
  end

  def test_report_lists_how_draws_failed
    a = results("results-a.json")
    want = "Draw.rect(10.0, 20.0, 30.0, 6.0, Color(40, 40, 60, 255))"
    runs = [
      ["Draw.rect(20.0, 10.0, 30.0, 6.0, Color(40, 40, 60, 255))", "differs"],
      ["Draw.rect(25.0, 23.0, 30.0, 6.0, Color(40, 40, 60, 255))", "differs"],
      [nil, "missing"]
    ].each_with_index.map do |(got, how), i|
      { "task" => "poster", "model" => "haiku", "trial" => i + 1, "attempt" => 1, "counts" => true,
        "outcome" => "fail", "checks" => { "build" => true, "output" => true, "draws" => false },
        "failed_checks" => ["draws"], "draws" => { "answer" => 1, "how" => how, "got" => got, "want" => want } }
    end
    a = a.merge("runs" => a["runs"] + runs, "task_names" => a["task_names"] + ["poster"])
    text = Eval::Report.render(a)
    assert_includes text, "## Draws"
    assert_match(/\| `poster` \| `haiku` \| 3 \| 3 \| 0 \| 0 \| 1 \| 0 \| 0 \| 2 \| 0 \|/, text)
    assert_includes text, "- Two numbers exchanged: poster/haiku/1.1\n"
    assert_includes text, "- A rectangle placed by its centre: poster/haiku/2.1\n"
    refute_includes Eval::Report.render(results("results-a.json")), "## Draws"
  end
end
