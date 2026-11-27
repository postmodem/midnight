# frozen_string_literal: true

require "minitest/autorun"
require "json"
require "fileutils"
require_relative "../eval"

# The agent's `midnight`, called directly with no agent, in front of a stub
# compiler that prints its arguments and environment.
class WrapperTest < Minitest::Test
  STUB = <<~'RUBY'
    require "json"
    if ARGV.first == "sleep"
      sleep 60
    elsif ARGV.first == "flood"
      $stdout.write("x" * 3_000_000)
      $stderr.write("y" * 3_000_000)
    else
      puts JSON.generate("args" => ARGV, "env" => ENV.to_h)
    end
  RUBY

  def setup
    @root, @wrapper, @logs, @other = 4.times.map { Eval::Env.mktemp }
    @stub = File.join(@other, "stub")
    File.write(@stub, "#!#{Eval::Env.ruby} --disable-gems\n#{STUB}")
    File.chmod(0o755, @stub)
    File.write(File.join(@root, "solution.12am"), "puts 1\n")
    File.write(File.join(@other, "outside.12am"), "puts 2\n")
    File.write(File.join(@other, "commands.drive"), "tick\n")
    @midnight = Eval.write_wrapper(@wrapper, root: @root, logs: @logs, compiler: @stub, limit: 3, cap: 1 << 20)
  end

  def teardown
    [@root, @wrapper, @logs, @other].each { |d| FileUtils.rm_rf(d) }
  end

  def call(*args, env: {}, chdir: @root)
    out_r, out_w = IO.pipe
    err_r, err_w = IO.pipe
    pid = Process.spawn(env, @midnight, *args, chdir: chdir, out: out_w, err: err_w, unsetenv_others: true)
    out_w.close
    err_w.close
    reader = Thread.new { out_r.read }
    err = err_r.read
    out = reader.value
    Process.wait(pid)
    [$?.exitstatus, out, err]
  end

  def log = Eval::CallLog.read(File.join(@logs, "calls.jsonl"))

  def stub_env(out) = JSON.parse(out)["env"]

  def assert_refused(*args, env: {})
    status, out, err = call(*args, env: env)
    assert_equal 2, status
    assert_empty out
    assert_equal 1, err.lines.size
    assert_match(/\Amidnight: refused: /, err)
    assert log.last["refused"], "the call log records the refusal"
    assert_equal 2, log.last["status"]
  end

  def test_runs_the_compiler_with_its_own_environment
    status, out, = call("check", "solution.12am")
    assert_equal 0, status
    env = stub_env(out)
    assert_equal %w[HOME MIDNIGHT_GPU MIDNIGHT_WINDOW PATH TMPDIR XDG_RUNTIME_DIR], env.keys.sort
    assert_equal "off", env["MIDNIGHT_GPU"]
    assert_equal "headless", env["MIDNIGHT_WINDOW"]
    assert Guard.inside?(env["TMPDIR"], @logs), "the compiler's TMPDIR is under the log directory"
    assert File.directory?(env["TMPDIR"])
    assert Dir.empty?(env["XDG_RUNTIME_DIR"])
    entry = log.last
    assert_equal %w[check solution.12am], entry["args"]
    assert_equal "solution.12am", entry["file"]
    assert_equal Digest::SHA256.file(File.join(@root, "solution.12am")).hexdigest, entry["sha256"]
    assert_equal 0, entry["status"]
    assert_kind_of Float, entry["load"]
  end

  def test_keeps_the_program_at_each_check_build_and_run
    FileUtils.mkdir_p(File.join(@root, "lib", "docs"))
    FileUtils.mkdir_p(File.join(@root, "docs"))
    File.write(File.join(@root, "lib", "util.12am"), "def f = 1\n")
    File.write(File.join(@root, "docs", "ref.12am"), "puts 3\n")
    File.write(File.join(@root, "notes.txt"), "not a program\n")
    call("check", "solution.12am")
    first = log.last["program_sha256"]
    kept = File.join(@logs, "programs", first)
    assert_equal ["lib/util.12am", "solution.12am"], Dir.glob("**/*", base: kept).select { |f| File.file?(File.join(kept, f)) }.sort
    assert_equal "puts 1\n", File.read(File.join(kept, "solution.12am"))
    call("build", "solution.12am", "-o", "x")
    assert_equal first, log.last["program_sha256"]
    File.write(File.join(@root, "solution.12am"), "puts 4\n")
    call("run", "solution.12am")
    call("loops", "solution.12am")
    assert_equal [first, log[-2]["program_sha256"]].sort, Dir.children(File.join(@logs, "programs")).sort
    assert_equal "puts 4\n", File.read(File.join(@logs, "programs", log[-2]["program_sha256"], "solution.12am"))

    keep = File.join(@other, "keep")
    Eval.keep_compiled(@logs, keep)
    assert_equal 2, Dir.children(File.join(keep, "programs")).size
  end

  def test_drops_the_compiler_variables_and_says_so
    vars = %w[MIDNIGHT_CC MIDNIGHT_CFLAGS MIDNIGHT_GPU_FLAGS MIDNIGHT_WINDOW_FLAGS]
    vars.each do |name|
      status, out, = call("build", "solution.12am", env: { name => "/bin/false" })
      assert_equal 0, status
      refute stub_env(out).key?(name), "#{name} reaches the compiler"
      assert_includes log.last["dropped"], name
      assert_equal "/bin/false", log.last["caller"][name]
    end
  end

  def test_overrides_the_gpu_and_the_window
    _, out, = call("run", "solution.12am", env: { "MIDNIGHT_GPU" => "on" })
    assert_equal "off", stub_env(out)["MIDNIGHT_GPU"]
    assert_equal "on", log.last["caller"]["MIDNIGHT_GPU"]
    assert_equal "off", log.last["passed"]["MIDNIGHT_GPU"]

    _, out, = call("run", "solution.12am", env: { "MIDNIGHT_WINDOW" => "" })
    assert_equal "headless", stub_env(out)["MIDNIGHT_WINDOW"]
    assert_equal "", log.last["caller"]["MIDNIGHT_WINDOW"]

    _, out, = call("run", "solution.12am", env: { "MIDNIGHT_WINDOW" => "info" })
    assert_equal "info,headless", stub_env(out)["MIDNIGHT_WINDOW"]
  end

  def test_passes_threads_and_commands
    File.write(File.join(@root, "c.drive"), "tick\n")
    _, out, = call("run", "solution.12am", env: { "MIDNIGHT_THREADS" => "4", "MIDNIGHT_DRIVE" => "c.drive" })
    env = stub_env(out)
    assert_equal "4", env["MIDNIGHT_THREADS"]
    assert_equal "c.drive", env["MIDNIGHT_DRIVE"]
  end

  def test_passes_the_format
    _, out, = call("run", "solution.12am", "--format", "json", env: { "MIDNIGHT_FORMAT" => "json" })
    assert_equal "json", stub_env(out)["MIDNIGHT_FORMAT"]
    refute_includes log.last["dropped"], "MIDNIGHT_FORMAT"
    refute log.last["refused"], "`json` after `--format` is not a file"
  end

  def test_refuses_a_file_outside
    assert_refused("check", File.join(@other, "outside.12am"))
    assert_refused("check", "../#{File.basename(@other)}/outside.12am")
  end

  def test_refuses_an_output_outside
    assert_refused("build", "solution.12am", "-o", File.join(@other, "prog"))
    assert_refused("build", "solution.12am", "-o", "../#{File.basename(@other)}/prog")
  end

  def test_refuses_commands_outside
    assert_refused("run", "solution.12am", env: { "MIDNIGHT_DRIVE" => File.join(@other, "commands.drive") })
    assert_refused("run", "solution.12am", env: { "MIDNIGHT_DRIVE" => "../#{File.basename(@other)}/commands.drive" })
  end

  def test_refuses_a_link_that_leads_outside
    File.symlink(File.join(@other, "outside.12am"), File.join(@root, "link.12am"))
    assert_refused("check", "link.12am")
  end

  def test_refuses_a_require_outside
    File.write(File.join(@root, "solution.12am"), "require \"../#{File.basename(@other)}/outside\"\nputs 1\n")
    assert_refused("check", "solution.12am")
    File.write(File.join(@root, "solution.12am"), "require \"#{@other}/outside\"\n")
    assert_refused("check", "solution.12am")
  end

  def test_refuses_while_any_file_requires_outside
    FileUtils.mkdir_p(File.join(@root, "lib"))
    File.write(File.join(@root, "lib", "helper.12am"), "require \"../../x\"\n")
    assert_refused("check", "solution.12am")
  end

  def test_refuses_every_form_of_a_require_outside
    name = File.basename(@other)
    [
      "require '../#{name}/outside'",
      "require\"../#{name}/outside\"",
      "require '#{@other}/outside'",
      "  require \"#{@other}/outside\"  # a comment",
      "require \"..\\/#{name}/outside\"",
      "require \"a\#{1}\"",
      "require '../#{name}/outside' x"
    ].each do |line|
      File.write(File.join(@root, "solution.12am"), "#{line}\nputs 1\n")
      assert_refused("check", "solution.12am")
    end
  end

  def test_refuses_a_require_through_a_link
    FileUtils.mkdir_p(File.join(@other, "sub"))
    File.symlink(File.join(@other, "sub"), File.join(@root, "deep"))
    File.write(File.join(@root, "solution.12am"), "require \"deep/../outside\"\n")
    assert_refused("check", "solution.12am")
    File.write(File.join(@root, "solution.12am"), "require \"deep/x\"\n")
    assert_refused("check", "solution.12am")
  end

  def test_accepts_a_require_inside
    File.write(File.join(@root, "helper.12am"), "def two -> Int\n  2\nend\n")
    FileUtils.mkdir_p(File.join(@root, "sub"))
    File.write(File.join(@root, "sub", "three.12am"), "require '../helper'\n")
    ["require \"helper\"", "require 'helper' # two", "require \"sub/three\"", "require \"~nobody/x\""].each do |line|
      File.write(File.join(@root, "solution.12am"), "#{line}\nputs two\n")
      status, = call("check", "solution.12am")
      assert_equal 0, status, line
    end
  end

  def test_refuses_variables_that_change_how_it_starts
    ran = File.join(@other, "ran")
    File.write(File.join(@root, "x.rb"), "File.write(#{ran.inspect}, '')\n")
    File.write(File.join(@root, "json.rb"), "File.write(#{ran.inspect}, '')\n")
    [
      { "MIDNIGHT_A" => "1", "RUBYOPT" => "-r./x.rb" },
      { "MIDNIGHT_WINDOW" => "headless", "RUBYOPT" => "-r./x.rb" },
      { "RUBYLIB" => "." },
      { "RUBY_DEBUG" => "1" },
      { "LD_PRELOAD" => "" },
      { "LD_BIND_NOW" => "1" },
      { "LD_LIBRARY_PATH" => "." },
      { "LD_LIBRARY_PATH" => ":/nix/store/x-lib/lib" },
      { "GCONV_PATH" => "." }
    ].each do |env|
      assert_refused("check", "solution.12am", env: env)
      refute File.exist?(ran), "#{env} ran code"
    end
  end

  def test_accepts_a_library_path_in_the_store
    status, = call("check", "solution.12am", env: { "LD_LIBRARY_PATH" => "/nix/store/x-alsa-lib/lib" })
    assert_equal 0, status
  end

  def test_resolves_from_the_callers_directory
    FileUtils.mkdir_p(File.join(@root, "sub"))
    status, = call("check", "../solution.12am", chdir: File.join(@root, "sub"))
    assert_equal 0, status
    assert_equal "sub", log.last["cwd"]
  end

  def test_stops_a_call_at_the_time_limit
    status, _, err = call("sleep")
    assert_equal 124, status
    assert_match(/stopped after 3 seconds/, err)
    assert log.last["timeout"]
  end

  def test_cuts_output_at_the_cap
    status, out, err = call("flood")
    assert_equal 0, status
    assert_equal 1 << 20, out.bytesize
    assert_equal 1 << 20, err.bytesize
    assert log.last["stdout_cut"]
    assert log.last["stderr_cut"]
  end
end
