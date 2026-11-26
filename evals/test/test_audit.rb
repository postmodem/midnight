# frozen_string_literal: true

require "minitest/autorun"
require_relative "../eval"

# The audit: which paths the calls that ran reached.
class AuditTest < Minitest::Test
  ROOT = "/tmp/eval.Root0000"
  WRAPPER = "/tmp/eval.Wrap0000"

  def words(command) = Eval::Audit.bash_words(command, ROOT)[0].map(&:first)
  def review(command) = Eval::Audit.bash_words(command, ROOT)[1]

  def test_bash_words
    assert_equal [], words("midnight check solution.12am && cat docs/language.md")
    assert_equal ["..", "/dev/null"], words("ls .. 2>/dev/null")
    assert_equal ["../y"], words("sed -n '/^## Lists/,/^## /p' ../y")
    assert_equal [], words("grep -n '/^x' docs/language.md | head -5")
    assert_equal ["/etc/passwd"], words("grep -e '..' /etc/passwd")
    assert_equal ["../d"], words("MIDNIGHT_DRIVE=../d midnight run solution.12am")
    assert_equal [], words("MIDNIGHT_DRIVE=d MIDNIGHT_CC=/bin/false LD_LIBRARY_PATH=/tmp midnight run solution.12am")
    assert_equal ["/etc/x"], words("RIPGREP_CONFIG_PATH=/etc/x rg foo docs")
    assert_equal ["/tmp/x"], words("cat a.12am>/tmp/x")
    assert_equal ["~/.ssh"], words("ls ~/.ssh;echo /not/a/path")
    assert_equal ["/bin/ls"], words("/bin/ls")
    assert_equal [], words("echo 'a | b ; ../c'")
  end

  def test_files_named_by_options
    assert_equal ["/outside/file"], words("grep -f /outside/file docs/x")
    assert_equal ["/x"], words("grep --file=/x docs")
    assert_equal ["/y"], words("rg --ignore-file /y foo docs")
    assert_equal ["~"], words("cd; ls")
  end

  def test_what_the_audit_cannot_follow
    assert_empty review("sed -n '/^## Lists/,/^## /p' docs/language.md")
    assert_empty review("awk 'NR>=10 && NR<=20' docs/language.md")
    refute_empty review("sed 'r /etc/passwd' docs/x")
    refute_empty review("sed -n '1w /tmp/x' docs/x")
    refute_empty review("sed 's/a/b/w out' docs/x")
    refute_empty review("awk '{ print > \"/tmp/x\" }' docs/x")
    refute_empty review("awk 'BEGIN { system(\"ls\") }'")
    refute_empty review("cat \"$HOME/.claude.json\"")
    refute_empty review("rg --pre=./x.sh y docs")
    refute_empty review("find . -exec cat {} ;")
  end

  def call(tool, input, denied: false, result: "") = { "tool" => tool, "input" => input, "denied" => denied, "result" => result }

  def audit(calls)
    Eval::Audit.check(calls, root: ROOT, wrapper: WRAPPER, tool_results: "#{Dir.home}/.claude/projects/-tmp-eval-Root0000")
  end

  def escapes(calls) = audit(calls)["escapes"]

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
      call("Bash", { "command" => "ls ~" }),
      call("Bash", { "command" => "ls ~+ ~nobody" })
    ]
    assert_equal ["/home/x/CLAUDE.md", "../secret", "/home/", "/etc", "~", "~+", "~nobody"], escapes(calls).map { |e| e["path"] }
  end

  def test_denied_calls_do_not_count
    assert_empty escapes([call("Bash", { "command" => "ls /home" }, denied: true)])
  end

  # Claude Code keeps the directory a `cd` left for the next Bash call,
  # unless the result says it reset it.
  def test_the_directory_carries_over
    calls = [call("Bash", { "command" => "cd docs" }), call("Bash", { "command" => "cat ../TASK.md" })]
    assert_empty escapes(calls)
    calls = [call("Bash", { "command" => "cd docs" }), call("Bash", { "command" => "cat ../../x" })]
    assert_equal ["../../x"], escapes(calls).map { |e| e["path"] }
    reset = "a\nShell cwd was reset to #{ROOT}"
    calls = [call("Bash", { "command" => "cd docs && ls" }, result: reset), call("Bash", { "command" => "cat ../x" })]
    assert_equal ["../x"], escapes(calls).map { |e| e["path"] }
    calls = [call("Bash", { "command" => "cd -" }), call("Bash", { "command" => "cat x" })]
    assert_empty escapes(calls)
    refute_empty audit(calls)["review"]
  end

  def test_an_odd_call_does_not_stop_the_audit
    bad = call("Bash", { "command" => "cat x" })
    bad["input"] = nil
    a = audit([bad, call("Bash", { "command" => "cat /etc/x" })])
    assert_equal ["/etc/x"], a["escapes"].map { |e| e["path"] }
    assert_match(/audit failed/, a["review"].first["what"])
  end
end
