# frozen_string_literal: true

require "minitest/autorun"
require "json"
require_relative "../eval"

# Kits, tasks, the compiler's standard error, call logs and transcripts.
class ParseTest < Minitest::Test
  FIXTURES = File.join(__dir__, "fixtures")

  def fixture(name) = File.read(File.join(FIXTURES, name))

  def test_kit_lines
    entries = Eval::Kit.parse("# docs\n\ndocs/language.md language.md\nexamples  examples\n")
    assert_equal [%w[docs/language.md language.md], %w[examples examples]], entries
    assert_raises(Eval::Failure) { Eval::Kit.parse("a ../b\n") }
    assert_raises(Eval::Failure) { Eval::Kit.parse("a /b\n") }
    assert_raises(Eval::Failure) { Eval::Kit.parse("a\n") }
  end

  def test_kit_files_expand_directories
    tree = %w[README.md docs/language.md examples/a.12am examples/a.out examples/sub/b.12am]
    files = Eval::Kit.files([%w[docs/language.md language.md], %w[examples ex]], tree)
    assert_equal({ "language.md" => "docs/language.md", "ex/a.12am" => "examples/a.12am",
                   "ex/a.out" => "examples/a.out", "ex/sub/b.12am" => "examples/sub/b.12am" }, files)
    assert_raises(Eval::Failure) { Eval::Kit.files([%w[nothing x]], tree) }
  end

  def test_sections
    text = "# Title\n\nintro\n\n## One\n\n```\n## not a heading\n```\n\n### Two\n\n#### Deeper\n"
    assert_equal [[5, "One"], [11, "Two"]], Eval::Kit.sections(text)
    assert_equal [[1, nil]], Eval::Kit.sections("no headings\n")
  end

  def test_meta
    meta = Eval::Task.parse_meta("areas: ecs strings\ncontrol: yes\nruby:\n")
    assert_equal({ "areas" => "ecs strings", "control" => "yes", "ruby" => "" }, meta)
    assert_raises(Eval::Failure) { Eval::Task.parse_meta("no colon\n") }
  end

  def test_task_loops
    dir = Eval::Env.mktemp
    File.write(File.join(dir, "meta"), "areas: cores\n")
    File.write(File.join(dir, "loops"), "system walk cores\nstrict gpu system settle gpu\nsystem count one core\n")
    task = Eval::Task.new(dir)
    assert_equal [["system walk", "cores"], ["strict gpu system settle", "gpu"], ["system count", "one core"]], task.loops
    assert_equal 15, task.minutes
  ensure
    FileUtils.rm_rf(dir)
  end

  def test_systems_check
    dir = Eval::Env.mktemp
    File.write(File.join(dir, "meta"), "areas: ecs\n")
    File.write(File.join(dir, "systems"), "hunt each\nreport\n")
    assert_equal [["hunt", true], ["report", false]], Eval::Task.new(dir).systems
    File.write(File.join(dir, "systems"), "hunt all\n")
    assert_raises(Eval::Failure) { Eval::Task.new(dir).systems }

    prog = File.join(dir, "prog")
    FileUtils.mkdir_p(File.join(prog, "lib"))
    File.write(File.join(prog, "solution.12am"), "require \"lib/more\"\nsystem hunt\nend\n# system report\n")
    File.write(File.join(prog, "lib", "more.12am"), "  gpu system settle\nend\n")
    loops = "solution.12am:3  system hunt  each Fox  one core: prints at line 4\n"
    wanted = [["hunt", true], ["settle", false], ["report", false]]
    assert_equal ["report"], Eval::Grade.missing_systems(prog, loops, wanted)
    assert_equal ["hunt each", "report"], Eval::Grade.missing_systems(prog, nil, wanted)

    File.write(File.join(prog, "solution.12am"), "system hunt\n  chase(1)\nend\n\nsystem report\n  puts 1\nend\n")
    helper = "solution.12am:9  def chase  each Fox  cores\n"
    assert_empty Eval::Grade.missing_systems(prog, helper, [["hunt", true], ["report", false]])
    assert_equal ["report each"], Eval::Grade.missing_systems(prog, helper, [["report", true]])
  ensure
    FileUtils.rm_rf(dir)
  end

  def test_loops_check
    out = "s.12am:4  system walk   each Pos  cores\n" \
          "s.12am:9  system walk   each Vel  one core: assigns `best` at line 10\n" \
          "s.12am:12  system count  each Pos  one core: prints at line 13\n"
    assert Eval::Grade.loops_ok?(out, [["system count", "one core"]])
    refute Eval::Grade.loops_ok?(out, [["system walk", "cores"]])
    refute Eval::Grade.loops_ok?(out, [["system missing", "cores"]])
  end

  def test_diagnostics
    err = <<~TEXT
      error: unknown method `insert` for `[Row]`
        --> solution.12am:80:12
         |
      80 |     sorted.insert(i, r)
         |            ^^^^^^
      error: component `Health` has fields without defaults
        --> lib/a.12am:10:7
      solution.12am:9: runtime error: 999 does not fit in UInt8
      midnight: MIDNIGHT_GPU_FLAGS is not set; GPU systems will run on the CPU
      note: first defined here
    TEXT
    d = Eval::Stderr.parse(err)
    assert_equal %w[error error runtime midnight], d.map { |x| x["kind"] }
    assert_equal "unknown method `…` for `…`", d[0]["template"]
    assert_equal ["insert", "[Row]"], d[0]["spans"]
    assert_equal ["solution.12am", 80], [d[0]["file"], d[0]["line"]]
    assert_equal ["lib/a.12am", 10], [d[1]["file"], d[1]["line"]]
    assert_equal "999 does not fit in UInt8", d[2]["template"]
    assert_equal ["solution.12am", 9], [d[2]["file"], d[2]["line"]]
    assert_equal "MIDNIGHT_GPU_FLAGS is not set; GPU systems will run on the CPU", d[3]["template"]
    assert Eval::Stderr.compiler_bug?("midnight: the C compiler rejected the generated code. This is a bug in midnight.\n")
  end

  def call(args, status, stderr: "", sha: "a", refused: nil, timeout: false)
    { "args" => args, "status" => status, "stderr" => stderr, "sha256" => sha,
      "file" => args[1], "refused" => refused, "timeout" => timeout }
  end

  def test_call_log_summary_and_repeats
    e1 = "error: unknown function `puts_all`\n  --> solution.12am:3:1\n"
    e2 = "error: unknown function `puts_all`\n  --> solution.12am:4:1\n"
    calls = [
      call(%w[check solution.12am], 1, stderr: e1),
      call(%w[run solution.12am], 1, stderr: e2),
      call(%w[run solution.12am], 1, stderr: "solution.12am:2: runtime error: division by zero\n"),
      call(%w[run solution.12am], 2, refused: "file `/x` is outside this directory"),
      call(%w[run solution.12am], 124, timeout: true),
      call(%w[build solution.12am -o s], 2, stderr: "midnight: unknown option `-x` for `build`\n"),
      call(%w[run solution.12am], 0)
    ]
    s = Eval::CallLog.summary(calls)
    assert_equal 7, s["total"]
    assert_equal({ "check" => 1, "run" => 5, "build" => 1 }, s["by_subcommand"])
    assert_equal({ "compile" => 2, "runtime" => 1, "refused" => 1, "timeout" => 1, "other" => 1 }, s["failed"])
    d = Eval::CallLog.diagnostics(calls)
    assert_equal [true, false], d.select { |x| x["template"] == "unknown function `…`" }.map { |x| x["repeated"] }
  end

  def test_first_try_and_verified
    calls = [
      call(%w[loops solution.12am], 0, sha: "w1"),
      call(%w[check solution.12am], 1, sha: "w1"),
      call(%w[run solution.12am], 0, sha: "w2"),
      call(%w[run solution.12am], 0, sha: "w3")
    ]
    assert_equal false, Eval::CallLog.first_try(calls, nil)
    assert Eval::CallLog.verified(calls, "w2")
    refute Eval::CallLog.verified(calls, "w4")
    assert_nil Eval::CallLog.first_try([], nil)
    # For a repair, calls on the given content do not count.
    assert_equal true, Eval::CallLog.first_try(calls.drop(1), "w1")
  end

  def test_init_isolation
    init = JSON.parse(fixture("init.json"))
    assert_nil Eval::Transcript.isolation_problem(init)
    assert_match(/tools/, Eval::Transcript.isolation_problem(init.merge("tools" => %w[Bash Read Agent])))
    assert_match(/MCP/, Eval::Transcript.isolation_problem(init.merge("mcp_servers" => [{ "name" => "x" }])))
    assert_match(/skills/, Eval::Transcript.isolation_problem(init.merge("skills" => ["dataviz"])))
    assert_match(/slash/, Eval::Transcript.isolation_problem(init.merge("slash_commands" => ["loop"])))
    mine = init["plugins"] + [{ "name" => "mine", "path" => "/home/x/.claude/plugins/mine", "source" => "mine@local" }]
    assert_match(/plugins \["mine"\]/, Eval::Transcript.isolation_problem(init.merge("plugins" => mine)))
    assert_equal "no init line", Eval::Transcript.isolation_problem(nil)
  end

  def test_tool_calls_and_denials
    t = Eval::Transcript.new(fixture("denied.jsonl"))
    calls = t.tool_calls
    assert_equal %w[Bash Read Bash Write], calls.map { |c| c["tool"] }
    assert_equal [true, true, false, false], calls.map { |c| c["denied"] }
    assert_equal "hi", calls[2]["result"]
    assert_equal ["claude-haiku-5-5"], t.models_reported
    assert_nil t.infrastructure
    u = t.usage
    assert_equal 5, u["turns"]
    refute u["partial"]
  end

  # A real run: Sonnet on thermostat, ids and the run's directory replaced.
  def test_recorded_run
    t = Eval::Transcript.new(fixture("thermostat-sonnet.jsonl"))
    assert_nil Eval::Transcript.isolation_problem(t.init)
    calls = t.tool_calls
    assert_equal 2, calls.count { |c| c["denied"] }
    assert(calls.select { |c| c["denied"] }.all? { |c| c["tool"] == "Bash" })
    assert_equal ["claude-sonnet-5-5"], t.models_reported
    assert_nil t.infrastructure
    root = t.init["cwd"]
    assert_empty Eval::Audit.escapes(calls, root: root, wrapper: "/tmp/eval.Wrap0000",
                                            tool_results: Eval.tool_results_dir(root))

    log = fixture("thermostat-sonnet-calls.jsonl").lines.map { |l| JSON.parse(l) }
    s = Eval::CallLog.summary(log)
    assert_equal({ "run" => 2 }, s["by_subcommand"])
    assert_equal({ "compile" => 1 }, s["failed"])
    d = Eval::CallLog.diagnostics(log)
    assert_equal ["`…` is a keyword and can't be used as a name"], d.map { |x| x["template"] }
    assert_equal [["on"]], d.map { |x| x["spans"] }
    assert_equal false, Eval::CallLog.first_try(log, nil)
    assert Eval::CallLog.verified(log, log.last["sha256"])
  end

  def test_infrastructure
    api = Eval::Transcript.new(fixture("api-error.jsonl"))
    reason, limit = api.infrastructure
    assert_match(/\AAPI error/, reason)
    refute limit
    usage = Eval::Transcript.new(fixture("usage-limit.jsonl"))
    reason, limit = usage.infrastructure
    assert_match(/\Ausage limit/, reason)
    assert limit
    cut = Eval::Transcript.new(fixture("denied.jsonl").lines[0..3].join)
    assert_equal ["no result line", false], cut.infrastructure
    assert cut.usage["partial"]
    assert_operator cut.usage["output_tokens"], :>, 0
  end
end
