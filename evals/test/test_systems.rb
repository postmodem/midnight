# frozen_string_literal: true

require "minitest/autorun"
require "tmpdir"
require "fileutils"
require_relative "../eval"

# The order the `systems` check requires: the named systems in the order
# they run, and with `others after`, before any other.
class SystemsTest < Minitest::Test
  def setup
    @dir = Dir.mktmpdir("eval-systems")
  end

  def teardown
    FileUtils.rm_rf(@dir)
  end

  def write(name, text)
    path = File.join(@dir, name)
    FileUtils.mkdir_p(File.dirname(path))
    File.write(path, text)
  end

  def task(systems)
    dir = File.join(@dir, "task")
    FileUtils.mkdir_p(dir)
    File.write(File.join(dir, "meta"), "areas: draw\n")
    File.write(File.join(dir, "systems"), systems)
    Eval::Task.new(dir)
  end

  def order_note(wanted, others_after: false)
    Eval::Grade.systems_out_of_order(@dir, wanted.map { |n| [n, false] }, others_after)
  end

  def test_the_marker
    t = task("adjust\nreport each\nothers after\n")
    assert_equal [["adjust", false], ["report", true]], t.systems
    assert t.others_after?
    refute task("adjust\nreport\n").others_after?
  end

  def test_order_follows_requires
    write("solution.12am", "system a\nend\nrequire \"lib/more\"\nsystem d\nend\nrequire 'lib/more'\n")
    write("lib/more.12am", "gpu system b\nend\nrequire \"deep\"\n")
    write("lib/deep.12am", "strict gpu system c\nend\n")
    write("unused.12am", "system z\nend\n")
    assert_equal %w[a b c d], Eval::Grade.system_order(@dir)
  end

  def test_named_systems_in_their_order
    write("solution.12am", "system move\nend\nsystem launch\nend\nsystem report\nend\n")
    assert_nil order_note(%w[move report])
    assert_equal "runs `move`, `launch` in that order", order_note(%w[launch move])
  end

  def test_others_after_the_named_ones
    write("solution.12am", "system adjust\nend\nsystem show\nend\nsystem report\nend\nsystem extra\nend\n")
    assert_nil order_note(%w[adjust report])
    assert_equal "`show` runs before `report`", order_note(%w[adjust report], others_after: true)
    assert_nil order_note(%w[adjust show report], others_after: true)
  end

  def test_a_missing_system_is_left_to_the_other_rule
    write("solution.12am", "system report\nend\n")
    assert_nil order_note(%w[adjust report], others_after: true)
  end
end
