# frozen_string_literal: true

require "minitest/autorun"
require_relative "../eval"

# The audit: which paths the calls that ran reached.
class AuditTest < Minitest::Test
  ROOT = "/tmp/eval.Root0000"
  WRAPPER = "/tmp/eval.Wrap0000"

  def words(command) = Eval::Audit.bash_words(command)

  def test_bash_words
    assert_equal [], words("midnight check solution.12am && cat docs/language.md")
    assert_equal ["..", "/dev/null"], words("ls .. 2>/dev/null")
    assert_equal ["../y"], words("sed -n '/^## Lists/,/^## /p' ../y")
    assert_equal [], words("grep -n '/^x' docs/language.md | head -5")
    assert_equal ["/etc/passwd"], words("grep -e '..' /etc/passwd")
    assert_equal ["../d"], words("MIDNIGHT_DRIVE=../d midnight run solution.12am")
    assert_equal ["/tmp/x"], words("cat a.12am>/tmp/x")
    assert_equal ["~/.ssh"], words("ls ~/.ssh;echo /not/a/path")
    assert_equal ["/bin/ls"], words("/bin/ls")
    assert_equal [], words("echo 'a | b ; ../c'")
  end

  def call(tool, input, denied: false) = { "tool" => tool, "input" => input, "denied" => denied }

  def escapes(calls)
    Eval::Audit.escapes(calls, root: ROOT, wrapper: WRAPPER, tool_results: "#{Dir.home}/.claude/projects/-tmp-eval-Root0000")
  end

  def test_paths_inside_are_allowed
    calls = [
      call("Read", { "file_path" => "#{ROOT}/docs/language.md" }),
      call("Read", { "file_path" => "#{Dir.home}/.claude/projects/-tmp-eval-Root0000/s/tool-results/x.txt" }),
      call("Bash", { "command" => "ls #{WRAPPER}" }),
      call("Bash", { "command" => "midnight run solution.12am > /dev/null 2>/dev/stderr" }),
      call("Glob", { "pattern" => "docs/**/*.md" }),
      call("Grep", { "pattern" => "/^## ", "path" => "docs" }),
      call("Bash", { "command" => "cd sub && cat ../solution.12am" })
    ]
    assert_empty escapes(calls)
  end

  def test_paths_outside_escape
    calls = [
      call("Read", { "file_path" => "/home/x/CLAUDE.md" }),
      call("Bash", { "command" => "cat ../secret" }),
      call("Glob", { "pattern" => "/home/**/*.md" }),
      call("Grep", { "pattern" => "x", "path" => "/etc" }),
      call("Bash", { "command" => "ls ~" })
    ]
    assert_equal ["/home/x/CLAUDE.md", "../secret", "/home/", "/etc", "~"], escapes(calls).map { |e| e["path"] }
  end

  def test_denied_calls_do_not_count
    assert_empty escapes([call("Bash", { "command" => "ls /home" }, denied: true)])
  end
end
