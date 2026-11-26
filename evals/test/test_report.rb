# frozen_string_literal: true

require "minitest/autorun"
require "json"
require_relative "../eval"

# What was read, the Ruby-shaped rule, Fisher's test, report and compare.
class ReportTest < Minitest::Test
  ROOT = "/tmp/eval.Root0000"
  FIXTURES = File.join(__dir__, "fixtures")
  DOC = (1..40).map { |n| n == 10 ? "## Lists\n" : n == 30 ? "### Sorting\n" : "line #{n} of the reference\n" }.join
  EXAMPLE = "puts \"hello, world\"\nputs \"and goodbye\"\n"
  DOCS = { "language.md" => DOC, "examples/hello.12am" => EXAMPLE }.freeze

  def call(tool, input, result, denied: false)
    { "tool" => tool, "input" => input, "result" => result, "denied" => denied }
  end

  def read(*calls) = Eval::Reading.lines(calls, root: ROOT, docs: DOCS)

  def test_read_tool_ranges
    shown = (11..13).map { |n| format("%6d\t%s", n, "line #{n} of the reference") }.join("\n")
    assert_equal({ "language.md" => [11, 12, 13] },
                 read(call("Read", { "file_path" => "#{ROOT}/docs/language.md" }, shown)))
  end

  def test_shell_output_located_by_text
    out = "line 5 of the reference\nline 6 of the reference\n"
    assert_equal({ "language.md" => [5, 6] },
                 read(call("Bash", { "command" => "sed -n '5,6p' docs/language.md" }, out)))
    assert_equal({ "language.md" => [10, 30] },
                 read(call("Bash", { "command" => "grep -n '^##' docs/language.md" }, "10:## Lists\n30:### Sorting")))
    assert_equal({ "examples/hello.12am" => [1, 2] },
                 read(call("Bash", { "command" => "cat docs/examples/*.12am" }, EXAMPLE)))
  end

  def test_grep_tool_in_content_mode_only
    input = { "pattern" => "Lists", "path" => "docs", "output_mode" => "content", "-n" => true }
    assert_equal({ "language.md" => [10] },
                 read(call("Grep", input, "#{ROOT}/docs/language.md:10:## Lists")))
    assert_empty read(call("Grep", input.merge("output_mode" => "files_with_matches"), "#{ROOT}/docs/language.md"))
  end

  def test_errors_previews_and_denials_count_only_what_they_show
    preview = "Output too large (144.6KB). Full output saved to: /x/tool-results/a.txt\n\n" \
              "Preview (first 2KB):\nline 1 of the reference\nline 2 of the reference\n..."
    assert_equal({ "language.md" => [1, 2] },
                 read(call("Bash", { "command" => "cat docs/language.md" }, preview)))
    too_big = "File content (40000 tokens) exceeds maximum allowed tokens (25000)."
    assert_empty read(call("Read", { "file_path" => "#{ROOT}/docs/language.md" }, too_big))
    assert_empty read(call("Bash", { "command" => "cat docs/language.md" }, DOC, denied: true))
    assert_empty read(call("Bash", { "command" => "midnight run docs/examples/hello.12am" }, EXAMPLE))
  end

  def test_the_shells_directory_carries_over
    out = "line 5 of the reference\nline 6 of the reference\n"
    assert_equal({ "language.md" => [5, 6, 10] },
                 read(call("Bash", { "command" => "cd docs && grep -n '^#' language.md" }, "10:## Lists"),
                      call("Bash", { "command" => "sed -n 5,6p language.md" }, out)))
    reset = "10:## Lists\nShell cwd was reset to #{ROOT}"
    assert_equal({ "language.md" => [10] },
                 read(call("Bash", { "command" => "cd docs && grep -n '^#' language.md" }, reset),
                      call("Bash", { "command" => "sed -n 5,6p language.md" }, out)))
    assert_empty read(call("Bash", { "command" => "cat ~nobody/language.md ~+/x" }, out))
  end

  def test_ruby_shaped
    shaped = ->(msg) { Eval::RubyShaped.ruby_shaped?(Eval::Stderr.template(msg), Eval::Stderr.spans(msg)) }
    assert shaped.("unknown method `insert` for `[Row]`")
    assert shaped.("unknown method `upcase` for `String`")
    assert shaped.("unknown method `times` for `Int`")
    assert shaped.("unknown method `round` for `Float`")
    assert shaped.("unknown method `tap` for `Team`")
    refute shaped.("unknown method `frobnicate` for `[Row]`")
    assert shaped.("unknown function `format`")
    assert shaped.("unknown variable or function `nil`")
    assert shaped.("unknown function `Math.cbrt`")
    assert shaped.("unknown constant `Math::PI`")
    assert shaped.("unknown constant `Hash`")
    assert shaped.("unknown type `Symbol`")
    refute shaped.("unknown constant `Widget`")
    refute shaped.("expected a component, found `Vec2`")
  end

  def test_fisher
    assert_in_delta 2.0 / 252, Eval::Stats.fisher(5, 0, 0, 5), 1e-12
    assert_in_delta 1.0, Eval::Stats.fisher(3, 2, 2, 3), 1e-12
    assert_in_delta 0.002759, Eval::Stats.fisher(1, 9, 11, 3), 1e-6
    assert_in_delta 1.0, Eval::Stats.fisher(5, 0, 5, 0), 1e-12
  end

  def results(name) = JSON.parse(File.read(File.join(FIXTURES, name)))

  def test_report
    text = Eval::Report.render(results("results-a.json"))
    %w[Pass\ rates Tasks Diagnostics The\ reference\ as\ read Exceptions].each { |h| assert_includes text, "## #{h}" }
    assert_match(/\| `league` \| `haiku` \| 1\/1 \| yes \|/, text)
    assert_match(/\| `sonnet` \| 100\.0% ± 0\.0 \| 3 of 3 \|/, text)
    assert_match(/``…` is a keyword and can't be used as a name` \| 1 \| 1 \| 1 \| 0 \|  \| on ×1 \|/, text)
    assert_match(/\| `language.md` \| Lists \| 3 \|/, text)
    assert_match(/Denied tool calls: 6 in 5 runs/, text)

    b = Eval::Report.render(results("results-b.json"))
    assert_match(/\| `league` \| `haiku` \| 0\/1 \| no \|/, b)
    assert_match(/\| error \| `unknown method `…` for `…`` \| 1 \| 1 \| 0 \| 0 \| yes \| insert \/ \[Row\] ×1 \|/, b)
  end

  def test_compare
    text = Eval::Compare.render(results("results-a.json"), results("results-b.json"))
    assert_match(/\| `haiku` \| 100\.0% \| 66\.7% \| -33\.3% ± 33\.3 \|/, text)
    assert_match(/\| `league` \| `haiku` \| 1\/1 \| 0\/1 \| 1\.000 \|  \|/, text)
    assert_match(/Kit: A `evals\/kit`, B `evals\/test\/kit-cut`; changed: language.md/, text)
    assert_match(/0 marked \(p < 0\.1\)\. Chance alone would mark about 0\.6 of 6 rows/, text)
    assert_match(/\| error \| `unknown method `…` for `…`` \| 0 \| 1 \|/, text)
    assert_match(/\| error \| ``…` is a keyword and can't be used as a name` \| 1 \| 0 \|/, text)
  end
end
