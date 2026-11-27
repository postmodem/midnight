# frozen_string_literal: true

# The evals' harness: runs agents on the tasks under evals/tasks, grades
# their programs, and writes results and reports. Standard library only.
#
#   ruby evals/eval.rb run [--tasks A,B] [--models M,N] [--trials K]
#                          [--effort E] [--kit FILE] [--jobs J] [--label L]
#   ruby evals/eval.rb run --resume RESULTS
#   ruby evals/eval.rb grade [--run RESULTS] TASK FILE
#   ruby evals/eval.rb grade --run RESULTS
#   ruby evals/eval.rb self-test [TASK...]
#   ruby evals/eval.rb report RESULTS
#   ruby evals/eval.rb compare A B
#   ruby evals/eval.rb isolation

require "json"
require "digest"
require "fileutils"
require "shellwords"
require "time"
require "etc"
require "rbconfig"
require "securerandom"

# Transcripts, call logs, tasks and the documentation are UTF-8 whatever
# the locale says.
Encoding.default_external = Encoding::UTF_8

module Eval
  ROOT = File.expand_path("..", __dir__)
  TOOLS = %w[Bash Read Edit Write Glob Grep].freeze
  PROMPT = "Read TASK.md and do the task."
  LIMIT = 30
  CAP = 1 << 20
  BUILD_SECONDS = 5

  class Failure < StandardError; end

  # The guards the agent's `midnight` applies, and the limits grading uses.
  # This source is also the body of every wrapper the harness writes, so it
  # requires nothing outside Ruby's standard library and reads no file of
  # the repository.
  GUARD_LINE = __LINE__ + 2
  GUARD = <<~'RUBY'
    module Guard
      module_function

      # Runs argv in a process group of its own with exactly env, stdin from
      # /dev/null. Passes on and keeps at most cap bytes of each stream, and
      # kills the group after limit seconds. Returns a hash with :status
      # (124 on the limit, 128 + signal when killed), :out, :err, :out_cut,
      # :err_cut, :timeout, :ms.
      def capture(env, argv, chdir:, limit:, cap:, pass: false)
        out_r, out_w = IO.pipe
        err_r, err_w = IO.pipe
        start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        pid = Process.spawn(env, *argv, unsetenv_others: true, pgroup: true,
                            chdir: chdir, in: File::NULL, out: out_w, err: err_w)
        out_w.close
        err_w.close
        streams = { out: [out_r, pass ? $stdout : nil], err: [err_r, pass ? $stderr : nil] }
        result = { out: +"", err: +"", out_cut: false, err_cut: false, timeout: false }
        readers = streams.map do |key, (io, sink)|
          Thread.new do
            buf = result[key]
            while (chunk = io.readpartial(65_536) rescue nil)
              room = cap - buf.bytesize
              if room <= 0
                result[:"#{key}_cut"] = true
                next
              end
              part = chunk.byteslice(0, room)
              result[:"#{key}_cut"] = true if part.bytesize < chunk.bytesize
              buf << part
              if sink
                sink.write(part)
                sink.flush
              end
            end
            io.close
          end
        end
        waiter = Thread.new { Process.wait2(pid)[1] }
        status = waiter.join(limit) ? waiter.value : nil
        if status.nil?
          result[:timeout] = true
          kill_group(pid)
          waiter.join
        else
          kill_group(pid)
        end
        readers.each(&:join)
        result[:status] =
          if result[:timeout] then 124
          elsif status.exitstatus then status.exitstatus
          else 128 + status.termsig.to_i
          end
        result[:ms] = ((Process.clock_gettime(Process::CLOCK_MONOTONIC) - start) * 1000).round
        result
      end

      def kill_group(pid)
        Process.kill(:KILL, -pid)
      rescue Errno::ESRCH, Errno::EPERM
        nil
      end

      # The absolute path path names from dir, taken a part at a time as the
      # kernel does: a symbolic link is followed where it stands, so a `..`
      # after it leaves the link's target. Past the first part that does not
      # exist, the rest is joined as written. A leading `~` is a name.
      def resolve(path, dir, depth = 0)
        full = path.start_with?("/") ? path : "#{dir}/#{path}"
        parts = full.split("/").reject(&:empty?)
        cur = "/"
        parts.each_with_index do |part, i|
          next if part == "."

          if part == ".."
            cur = File.dirname(cur)
            next
          end
          probe = File.join(cur, part)
          if File.symlink?(probe) && !File.exist?(probe) && depth < 40
            return resolve([File.readlink(probe), *parts[i + 1..]].join("/"), cur, depth + 1)
          end
          return File.join(cur, *parts[i..]) unless File.exist?(probe)

          cur = File.realpath(probe)
        end
        cur
      end

      def inside?(path, dir)
        path == dir || path.start_with?(dir.end_with?("/") ? dir : "#{dir}/")
      end

      REQUIRE = /\A\s*require(?![A-Za-z0-9_?!])\s*/

      # What is wrong with the requires of a source file: each path that
      # leads outside root, and each require this guard cannot read.
      def bad_requires(file, root)
        File.foreach(file).filter_map do |line|
          m = line.match(REQUIRE) or next
          path = require_path(m.post_match)
          next "`#{m.post_match.strip}`, which this guard cannot read" unless path

          "`#{path}`, outside this directory" unless inside?(resolve("#{path}.12am", File.dirname(file)), root)
        end
      rescue SystemCallError, ArgumentError
        []
      end

      # The path of the string a require line goes on with, or nil unless it
      # is one plain string in either quote, with only a comment after it.
      def require_path(rest)
        quote = rest[0]
        return nil unless ["'", '"'].include?(quote)

        out = +""
        i = 1
        while i < rest.size
          c = rest[i]
          n = rest[i + 1]
          if c == quote
            tail = rest[i + 1..].strip
            return tail.empty? || tail.start_with?("#") ? out : nil
          elsif c == "\n" || (quote == '"' && c == "#" && n == "{")
            return nil
          elsif c == "\\" && quote == "'"
            if ["'", "\\"].include?(n)
              out << n
              i += 1
            else
              out << c
            end
          elsif c == "\\"
            return nil unless ["\\", '"', "#"].include?(n)

            out << n
            i += 1
          else
            out << c
          end
          i += 1
        end
        nil
      end

      # The first of the caller's variables that changes how Ruby or the
      # dynamic loader starts, or nil. LD_LIBRARY_PATH may name directories
      # of the Nix store, as Claude Code's launcher sets it.
      def bad_variable(env)
        env.keys.sort.find do |name|
          if name == "LD_LIBRARY_PATH"
            env[name].split(":", -1).any? { |d| !d.start_with?("/nix/store/") }
          else
            name.start_with?("RUBY", "LD_", "GCONV")
          end
        end
      end

      # The file arguments of a compiler call: [files, out].
      def file_args(argv)
        files = []
        out = nil
        rest = argv.drop(1)
        until rest.empty?
          arg = rest.shift
          if arg == "-o"
            out = rest.shift
          elsif !arg.start_with?("-")
            files << arg
          end
        end
        [files, out]
      end

      # Why a call must be refused, or nil. root is the run's directory.
      def refusal(argv, caller_env, cwd, root)
        variable = bad_variable(caller_env)
        return "the variable `#{variable}` is not allowed" if variable

        files, out = file_args(argv)
        named = files.map { |f| ["file", f] }
        named << ["-o", out] if out
        named << ["MIDNIGHT_DRIVE", caller_env["MIDNIGHT_DRIVE"]] if caller_env.key?("MIDNIGHT_DRIVE")
        named.each do |what, path|
          next if inside?(resolve(path, cwd), root)

          return "#{what} `#{path}` is outside this directory"
        end
        sources = Dir.glob("**/*.12am", base: root).map { |f| File.join(root, f) }
        sources += files.map { |f| resolve(f, cwd) }.select { |f| File.file?(f) }
        sources.uniq.each do |file|
          bad = bad_requires(file, root).first
          return "`#{File.basename(file)}` requires #{bad}" if bad
        end
        nil
      end

      # The compiler's environment: [env, dropped names].
      def environment(caller_env, config)
        words = caller_env.fetch("MIDNIGHT_WINDOW", "").split(",").map(&:strip)
        words = words.reject { |w| w.empty? || w == "headless" } + ["headless"]
        env = {
          "PATH" => config["path"],
          "HOME" => config["home"],
          "TMPDIR" => config["tmp"],
          "XDG_RUNTIME_DIR" => config["xdg"],
          "MIDNIGHT_GPU" => "off",
          "MIDNIGHT_WINDOW" => words.join(",")
        }
        %w[MIDNIGHT_THREADS MIDNIGHT_DRIVE].each do |name|
          env[name] = caller_env[name] if caller_env.key?(name)
        end
        kept = %w[MIDNIGHT_THREADS MIDNIGHT_DRIVE]
        dropped = caller_env.keys.reject { |k| kept.include?(k) }.sort
        [env, dropped]
      end

      def sha256(path)
        require "digest"
        File.file?(path) ? Digest::SHA256.file(path).hexdigest : nil
      end

      # The program in dir: its .12am files outside docs/, as sorted
      # relative paths.
      def program_files(dir)
        Dir.glob("**/*.12am", base: dir).reject { |f| f.start_with?("docs/") }.sort
      end

      # A hash of the program in dir: each file's path and contents.
      def program_sha256(dir)
        require "digest"
        d = Digest::SHA256.new
        program_files(dir).each { |f| d << "#{f}\0#{File.binread(File.join(dir, f))}\0" }
        d.hexdigest
      rescue SystemCallError
        nil
      end

      def load_average
        File.read("/proc/loadavg").split.first.to_f
      rescue SystemCallError
        nil
      end

      # The wrapper itself: guards, runs the compiler, appends to the log.
      def main(argv, config)
        require "json"
        caller_env = ENV.to_h
        cwd = Dir.pwd
        root = config["root"]
        env, dropped = environment(caller_env, config)
        files, = file_args(argv)
        file = files.first && resolve(files.first, cwd)
        record = {
          "time" => Time.now.utc.strftime("%Y-%m-%dT%H:%M:%S.%LZ"),
          "cwd" => inside?(cwd, root) ? cwd.delete_prefix(root).delete_prefix("/") : cwd,
          "args" => argv,
          "file" => file && (inside?(file, root) ? file.delete_prefix("#{root}/") : file),
          "sha256" => file && sha256(file),
          "program_sha256" => program_sha256(root),
          "passed" => env.reject { |k, _| %w[PATH HOME XDG_RUNTIME_DIR].include?(k) },
          "dropped" => dropped,
          "caller" => caller_env.select { |k, _| k.start_with?("MIDNIGHT_") },
          "load" => load_average
        }
        keep_program(root, config["programs"], record["program_sha256"]) if %w[check build run].include?(argv.first)
        reason = refusal(argv, caller_env, cwd, root)
        if reason
          line = "midnight: refused: #{reason}"
          $stderr.puts line
          record.merge!("status" => 2, "refused" => reason, "stderr" => "#{line}\n", "stdout" => "")
          append(config["log"], record)
          exit 2
        end
        r = capture(env, [config["compiler"], *argv], chdir: cwd, limit: config["limit"],
                    cap: config["cap"], pass: true)
        if r[:timeout]
          line = "midnight: stopped after #{config["limit"]} seconds"
          $stderr.puts line
          r[:err] << "#{line}\n"
        end
        record.merge!("status" => r[:status], "timeout" => r[:timeout], "ms" => r[:ms],
                      "stdout" => r[:out].scrub, "stderr" => r[:err].scrub,
                      "stdout_cut" => r[:out_cut], "stderr_cut" => r[:err_cut])
        append(config["log"], record)
        exit r[:status]
      end

      # Copies the program in root to programs/SHA/, at the same relative
      # paths, unless that program is already kept.
      def keep_program(root, programs, sha)
        return unless programs && sha

        dest = File.join(programs, sha)
        return if File.directory?(dest)

        require "fileutils"
        partial = "#{dest}.#{Process.pid}"
        program_files(root).each do |f|
          FileUtils.mkdir_p(File.dirname(File.join(partial, f)))
          FileUtils.cp(File.join(root, f), File.join(partial, f))
        end
        FileUtils.mkdir_p(partial)
        File.rename(partial, dest)
      rescue SystemCallError
        FileUtils.rm_rf(partial) if partial
      end

      def append(log, record)
        File.open(log, "a") do |f|
          f.flock(File::LOCK_EX)
          f.puts(JSON.generate(record))
        end
      end
    end
  RUBY
  Object.class_eval(GUARD, __FILE__, GUARD_LINE)

  # Where the tools the harness hands on come from: the flake app sets
  # these; elsewhere the caller's PATH is searched.
  module Env
    module_function

    def ruby
      ENV.fetch("EVAL_RUBY", RbConfig.ruby)
    end

    def which(name)
      ENV.fetch("PATH", "").split(":").each do |dir|
        path = File.join(dir, name)
        return File.realpath(path) if File.file?(path) && File.executable?(path)
      end
      nil
    end

    def tool_dir(name)
      path = which(name) or raise Failure, "`#{name}` is not on the PATH"
      File.dirname(path)
    end

    # The directories the agent's PATH ends with.
    def agent_tools
      ENV["EVAL_AGENT_TOOLS"] || %w[ls bash find grep sed rg].map { |t| tool_dir(t) }.uniq.join(":")
    end

    # The compiler's PATH inside the wrapper.
    def coreutils
      ENV["EVAL_COREUTILS"] || tool_dir("ls")
    end

    # Ruby's own $LOAD_PATH, with no RUBYLIB.
    def load_path
      @load_path ||= begin
        out = IO.popen({}, [ruby, "--disable=gems,rubyopt", "-e", "puts $LOAD_PATH"],
                       unsetenv_others: true, &:read)
        raise Failure, "`#{ruby}` failed" unless $?.success?

        out.lines.map(&:chomp)
      end
    end

    def mktemp
      out = IO.popen(["mktemp", "-d", "/tmp/eval.XXXXXXXX"], &:read).strip
      raise Failure, "mktemp failed" unless $?.success? && File.directory?(out)

      out
    end
  end

  # A task, read from a directory.
  class Task
    attr_reader :name, :dir, :meta

    def initialize(dir)
      @dir = dir
      @name = File.basename(dir)
      @meta = Task.parse_meta(File.read(File.join(dir, "meta")))
    end

    def self.parse_meta(text)
      text.each_line.with_object({}) do |line, h|
        line = line.chomp
        next if line.strip.empty?

        key, value = line.split(":", 2)
        raise Failure, "meta: no `:` in `#{line}`" unless value

        h[key.strip] = value.strip
      end
    end

    def path(*parts) = File.join(@dir, *parts)
    def read(*parts) = File.read(path(*parts))
    def exist?(*parts) = File.exist?(path(*parts))
    def minutes = (@meta["minutes"] || "15").to_i
    def areas = @meta.fetch("areas", "").split
    def expected = read("expected.out")
    def drive = exist?("check.drive") ? path("check.drive") : nil
    def repair? = given_files.include?("solution.12am")
    def draws? = exist?("expected.draws")
    def expected_draws = read("expected.draws")

    # Whether the shapes of each `draws` answer are compared as a set:
    # `draws: any order` in meta; `draws: in order` compares them line for
    # line.
    def draws_any_order?
      case @meta["draws"]
      when "any order" then true
      when "in order" then false
      else raise Failure, "#{@name}/meta: `draws:` is `in order` or `any order`"
      end
    end

    def given_files
      return [] unless exist?("given")

      Dir.glob("**/*", base: path("given")).select { |f| File.file?(path("given", f)) }.sort
    end

    def programs(kind)
      Dir.glob("*.12am", base: path(kind)).sort.map { |f| path(kind, f) }
    end

    # Each line of `systems` as [name, each?].
    # The systems a program must declare, in the order they run, as
    # [name, each]. A last line `others after` says that any other system
    # runs after them.
    def systems
      return [] unless exist?("systems")

      lines = read("systems").each_line.map(&:split).reject(&:empty?)
      lines.pop if lines.last == %w[others after]
      lines.map do |name, each, extra|
        raise Failure, "#{@name}/systems: `#{[name, each, extra].compact.join(' ')}` is not `NAME [each]`" if extra || (each && each != "each")

        [name, each == "each"]
      end
    end

    def others_after? = exist?("systems") && read("systems").each_line.map(&:split).reject(&:empty?).last == %w[others after]

    # Each line of `loops` as [where, place].
    def loops
      return [] unless exist?("loops")

      read("loops").each_line.map(&:strip).reject(&:empty?).map do |line|
        if line.end_with?(" one core")
          [line.delete_suffix(" one core"), "one core"]
        else
          where, _, place = line.rpartition(" ")
          [where, place]
        end
      end
    end

    # A hash of what the agent is given and graded against.
    def sha256
      d = Digest::SHA256.new
      %w[TASK.md meta expected.out check.drive loops systems].each do |f|
        d << "#{f}\0#{exist?(f) ? read(f) : ''}\0"
      end
      d << "expected.draws\0#{expected_draws}\0" if draws?
      given_files.each { |f| d << "given/#{f}\0#{read('given', f)}\0" }
      d.hexdigest
    end

    def self.all(root)
      Dir.glob("*/meta", base: root).map { |m| File.dirname(m) }.sort.map { |n| Task.new(File.join(root, n)) }
    end
  end

  # A kit: which documentation files the agent gets, and where.
  module Kit
    module_function

    def parse(text)
      text.each_line.map(&:strip).reject { |l| l.empty? || l.start_with?("#") }.map do |line|
        source, dest, extra = line.split(/\s+/)
        raise Failure, "kit: `#{line}` is not `SOURCE DEST`" unless dest && !extra
        if dest.start_with?("/") || dest.split("/").include?("..")
          raise Failure, "kit: `#{dest}` is not a relative path without `..`"
        end

        [source, dest]
      end
    end

    # The kit's files as { dest => source path } for a tree whose files
    # list_files returns.
    def files(entries, list_files)
      entries.each_with_object({}) do |(source, dest), out|
        under = list_files.select { |f| f.start_with?("#{source}/") }
        if list_files.include?(source)
          out[dest] = source
        elsif !under.empty?
          under.each { |f| out[File.join(dest, f.delete_prefix("#{source}/"))] = f }
        else
          raise Failure, "kit: `#{source}` is not in the tree"
        end
      end
    end

    BEFORE = "(before the first section)"

    # Each `##` and `###` heading of a Markdown file as [line, title], after
    # a section for the lines before the first one, below line 1, when any
    # of them has text; a file with none gets one section for the whole
    # file.
    def sections(text)
      fence = false
      found = []
      lines = text.lines
      lines.each.with_index(1) do |line, n|
        fence = !fence if line.start_with?("```")
        next if fence

        m = line.match(/\A(##|###) (.+)/)
        found << [n, m[2].strip] if m
      end
      return [[1, nil]] if found.empty?

      intro = lines[1...(found.first[0] - 1)].to_a.any? { |l| !l.strip.empty? }
      intro ? [[1, BEFORE], *found] : found
    end
  end

  # The repository at one commit.
  module Git
    module_function

    def run(*args)
      out = IO.popen(["git", "-C", ROOT, *args], err: File::NULL, &:read)
      raise Failure, "git #{args.join(' ')} failed" unless $?.success?

      out
    end

    def head = run("rev-parse", "HEAD").strip
    def files(commit)
      (@files ||= {})[commit] ||= run("ls-tree", "-r", "--name-only", commit).split("\n")
    end
    def show(commit, path) = run("show", "#{commit}:#{path}")

    def clean?(commit, path)
      system("git", "-C", ROOT, "diff", "--quiet", commit, "--", path, out: File::NULL, err: File::NULL)
    end

    # Writes path (a file or directory) at commit under dest.
    def export(commit, path, dest)
      files(commit).select { |f| f == path || f.start_with?("#{path}/") }.each do |f|
        target = File.join(dest, f)
        FileUtils.mkdir_p(File.dirname(target))
        File.binwrite(target, show(commit, f))
      end
    end
  end

  # The compiler's standard error, as records.
  module Stderr
    module_function

    def template(message) = message.gsub(/`[^`]*`/, "`…`")
    def spans(message) = message.scan(/`([^`]*)`/).flatten

    # Each diagnostic, run-time error and `midnight:` line. Of a `draws`
    # answer, its first line is recorded as `drive: draws`, and the lines
    # after it are not.
    def parse(text)
      lines = text.to_s.lines.map(&:chomp)
      out = []
      answer = false
      lines.each_with_index do |line, i|
        if line.start_with?("#{Draws::SAID}draws: ")
          answer = true
          out << record("midnight", "drive: draws", nil, nil)
          next
        end
        next if answer && line.start_with?("#{Draws::SAID}  ")

        answer = false
        if (m = line.match(/\Aerror: (.*)\z/))
          at = lines[i + 1].to_s.match(/\A\s*--> (.*):(\d+):\d+\z/)
          out << record("error", m[1], at && at[1], at && at[2].to_i)
        elsif (m = line.match(/\A(.*?):(\d+): runtime error: (.*)\z/))
          out << record("runtime", m[3], m[1], m[2].to_i)
        elsif line.start_with?("midnight: ")
          out << record("midnight", line.delete_prefix("midnight: "), nil, nil)
        end
      end
      out
    end

    def record(kind, message, file, line)
      { "kind" => kind, "message" => message, "template" => template(message),
        "spans" => spans(message), "file" => file, "line" => line }
    end

    def compiler_bug?(text) = text.to_s.include?("the C compiler rejected the generated code")
  end

  # The wrapper's call log.
  module CallLog
    module_function

    def read(path)
      return [] unless File.exist?(path)

      File.readlines(path).map { |l| JSON.parse(l) }
    end

    def subcommand(call) = call["args"].first.to_s

    # What a call ended in: ok, compile, runtime, other, timeout, refused.
    def outcome(call)
      return "refused" if call["refused"]
      return "timeout" if call["timeout"]
      return "ok" if call["status"].zero?

      parsed = Stderr.parse(call["stderr"])
      if parsed.any? { |d| d["kind"] == "error" } then "compile"
      elsif parsed.any? { |d| d["kind"] == "runtime" } then "runtime"
      else "other"
      end
    end

    # Counts of calls by subcommand, and of failures by kind.
    def summary(calls)
      by = Hash.new(0)
      failed = Hash.new(0)
      calls.each do |c|
        by[subcommand(c)] += 1
        o = outcome(c)
        failed[o] += 1 unless o == "ok"
      end
      { "total" => calls.size, "by_subcommand" => by, "failed" => failed,
        "failed_total" => failed.values.sum }
    end

    # Every diagnostic of the run, each marked repeated when the same
    # template and spans came up at a later call.
    def diagnostics(calls)
      all = calls.each_with_index.flat_map do |c, i|
        Stderr.parse(c["stderr"]).map { |d| d.merge("call" => i) }
      end
      all.each do |d|
        d["repeated"] = all.any? do |e|
          e["call"] > d["call"] && e["template"] == d["template"] && e["spans"] == d["spans"]
        end
      end
    end

    COMPILES = %w[check build run].freeze

    # Whether the first check, build or run of solution.12am with content
    # the agent wrote exited 0; nil when there was none.
    def first_try(calls, given_sha)
      first = calls.find do |c|
        !c["refused"] && COMPILES.include?(subcommand(c)) && c["file"] == "solution.12am" &&
          c["sha256"] && c["sha256"] != given_sha
      end
      first && first["status"].zero?
    end

    # Whether a run of solution.12am exited 0 with the graded content: the
    # whole program's, where the call log has its hash.
    def verified(calls, graded_sha, graded_program = nil)
      return false unless graded_sha

      calls.any? do |c|
        same = c["program_sha256"] && graded_program ? c["program_sha256"] == graded_program : c["sha256"] == graded_sha
        !c["refused"] && subcommand(c) == "run" && c["file"] == "solution.12am" && same && c["status"].zero?
      end
    end
  end

  # Claude Code's stream-json transcript.
  class Transcript
    DENIED = [/\APermission for this tool use was denied/, /--restricted confines the file tools/].freeze
    USAGE = /usage limit|hit your limit|limit reached|limit will reset|resets at/i
    API = /API Error|overloaded|internal server error|server_error|rate.?limit|ECONNRESET|socket hang up/i

    attr_reader :lines

    def initialize(text)
      @lines = text.each_line.filter_map do |l|
        JSON.parse(l)
      rescue JSON::ParserError
        nil
      end
    end

    def self.load(path) = new(File.exist?(path) ? File.read(path) : "")

    def init = @lines.find { |l| l["type"] == "system" && l["subtype"] == "init" }
    def result = @lines.reverse.find { |l| l["type"] == "result" }
    def version = init&.dig("claude_code_version")

    # Why the init line shows the flags no longer isolate, or nil. root is
    # the run's directory, when known.
    def self.isolation_problem(init, root = nil)
      return "no init line" unless init

      problems = []
      problems << "cwd #{init['cwd'].inspect}" if root && init["cwd"] != root
      problems << "permission mode #{init['permissionMode'].inspect}" unless init["permissionMode"] == "default"
      problems << "tools #{init['tools'].inspect}" unless init["tools"].to_a.sort == TOOLS.sort
      problems << "MCP servers #{init['mcp_servers'].inspect}" unless init["mcp_servers"].to_a.empty?
      problems << "skills #{init['skills'].inspect}" unless init["skills"].to_a.empty?
      problems << "slash commands #{init['slash_commands'].inspect}" unless init["slash_commands"].to_a.empty?
      outside = init["plugins"].to_a.reject { |p| p["path"] == "builtin" && p["source"].to_s.end_with?("@builtin") }
      problems << "plugins #{outside.map { |p| p['name'] }.inspect}" unless outside.empty?
      problems.empty? ? nil : problems.join(", ")
    end

    # Every tool call, in order, with its result: hashes with "id", "tool",
    # "input", "result" (text or nil), "error", "denied".
    def tool_calls
      @tool_calls ||= begin
        results = {}
        @lines.each do |l|
          next unless l["type"] == "user"

          Array(l.dig("message", "content")).each do |c|
            next unless c.is_a?(Hash) && c["type"] == "tool_result"

            results[c["tool_use_id"]] = c
          end
        end
        denied_ids = Array(result&.dig("permission_denials")).map { |d| d["tool_use_id"] }
        calls = []
        @lines.each do |l|
          next unless l["type"] == "assistant"

          Array(l.dig("message", "content")).each do |c|
            next unless c.is_a?(Hash) && c["type"] == "tool_use"

            r = results[c["id"]]
            text = r && Transcript.text(r["content"])
            denied = denied_ids.include?(c["id"]) ||
                     (r && r["is_error"] && DENIED.any? { |d| text.to_s.match?(d) })
            calls << { "id" => c["id"], "tool" => c["name"], "input" => c["input"] || {},
                       "result" => text, "error" => r ? r["is_error"] == true : nil,
                       "denied" => denied ? true : false }
          end
        end
        calls
      end
    end

    def self.text(content)
      case content
      when String then content
      when Array then content.filter_map { |c| c["text"] if c.is_a?(Hash) }.join("\n")
      end
    end

    # nil, or why the run is an infrastructure error: [reason, usage_limit?].
    # Every result with is_error is one: an agent that finishes never sets
    # it. A usage limit is told by its text; a rate limit is an API error.
    def infrastructure
      r = result
      return ["no result line", false] unless r
      return nil unless r["is_error"]

      text = r["result"].to_s
      return ["API error: #{text[0, 200]}", false] if text.match?(API)
      return ["usage limit: #{text[0, 200]}", true] if text.match?(USAGE)
      return ["API error: #{text[0, 200]}", false] if r["api_error_status"]

      ["error: #{text[0, 200]}", false]
    end

    # Turns, duration, tokens and cost.
    def usage
      r = result
      if r
        u = r["usage"] || {}
        { "turns" => r["num_turns"], "duration_ms" => r["duration_ms"],
          "input_tokens" => u["input_tokens"].to_i, "output_tokens" => u["output_tokens"].to_i,
          "cache_read_tokens" => u["cache_read_input_tokens"].to_i,
          "cache_creation_tokens" => u["cache_creation_input_tokens"].to_i,
          "cost_usd" => r["total_cost_usd"], "partial" => false }
      else
        msgs = @lines.select { |l| l["type"] == "assistant" }
        seen = {}
        totals = Hash.new(0)
        msgs.each do |m|
          id = m.dig("message", "id")
          seen[id] = m.dig("message", "usage") || {} if id
        end
        seen.each_value do |u|
          totals["input_tokens"] += u["input_tokens"].to_i
          totals["output_tokens"] += u["output_tokens"].to_i
          totals["cache_read_tokens"] += u["cache_read_input_tokens"].to_i
          totals["cache_creation_tokens"] += u["cache_creation_input_tokens"].to_i
        end
        { "turns" => seen.size, "duration_ms" => nil }.merge(totals)
                                                      .merge("cost_usd" => nil, "partial" => true)
      end
    end

    def models_reported = (result&.dig("modelUsage") || {}).keys.sort
  end

  # Which paths a run's tool calls reached.
  module Audit
    module_function

    REDIRECT = /\A\d*(?:&>>|&>|>>|>\||>&|>|<<<|<<|<&|<>|<)/
    PATTERN_FIRST = %w[grep egrep fgrep rg sed awk].freeze
    NO_PATHS = %w[echo printf].freeze
    FILE_OPTIONS = %w[-f --file --exclude-from --ignore-file].freeze
    RUNS = %w[--pre -exec -execdir -ok -okdir].freeze
    RESET = /Shell cwd was reset to (\S+)/

    # A shell command split into simple commands at unquoted `|`, `;`,
    # `&`, `&&`, `||` and newlines, with a space put before each unquoted
    # redirection that follows a word.
    def segments(command)
      out = [+""]
      quote = nil
      chars = command.chars
      chars.each_with_index do |ch, i|
        before = i.positive? ? chars[i - 1] : " "
        if quote
          quote = nil if ch == quote
          out.last << ch
        elsif ch == "'" || ch == '"'
          quote = ch
          out.last << ch
        elsif ch == "|" || ch == ";" || ch == "\n" ||
              (ch == "&" && !"<>".include?(before) && chars[i + 1] != ">")
          out << +""
        elsif "<>".include?(ch) && !before.match?(/[\s<>&\d]/)
          out.last << " " << ch
        else
          out.last << ch
        end
      end
      out.map(&:strip).reject(&:empty?)
    end

    def split(seg)
      Shellwords.split(seg)
    rescue ArgumentError
      seg.split(/\s+/)
    end

    # Each call that ran, with the shell's directory when it began (nil
    # when not known). Claude Code keeps the directory a Bash call's `cd`
    # left for the next one, unless the call's result says it reset it.
    def walk(calls, root)
      cwd = root
      calls.each do |c|
        next if c["denied"]

        yield c, cwd
        next unless c["tool"] == "Bash"

        cwd = bash_words(c.dig("input", "command").to_s, cwd)[2]
        reset = c["result"].to_s[RESET, 1]
        cwd = reset if reset
      end
    end

    # `~` and `~/…` as paths under the home directory; nil for any other
    # word.
    def tilde(word)
      return nil unless word == "~" || word.start_with?("~/")

      File.join(Dir.home, word.delete_prefix("~"))
    end

    # The absolute path of a word taken from dir: :outside for `~+`, `~-`
    # and `~user`, nil for a relative word when dir is not known.
    def expand(word, dir)
      home = tilde(word)
      return home if home
      return :outside if word.start_with?("~")
      return nil if dir.nil? && !word.start_with?("/")

      File.expand_path(word, dir || "/")
    end

    # The directory `cd` with these words goes to from dir, or nil when it
    # cannot be known.
    def cd(words, dir)
      target = words[1] || "~"
      return nil if target == "-" || target.match?(/[$`]/)

      full = expand(target, dir)
      full == :outside ? nil : full
    end

    # [found, review, dir]: each word of a shell command that may be a
    # path, as [word, the directory it is taken from, whether it is a word
    # of a `midnight` command]; what the audit cannot follow; and the
    # directory the command leaves the shell in. `cd` alone counts as `~`.
    def bash_words(command, cwd)
      dir = cwd
      found = []
      review = []
      segments(command).each do |seg|
        words = split(seg)
        paths, why = segment_words(words)
        compiler = File.basename(words.drop_while { |w| w.match?(/\A[A-Za-z_][A-Za-z_0-9]*=/) }.first.to_s) == "midnight"
        found.concat(paths.map { |w| [w, dir, compiler] })
        review.concat(why)
        next unless File.basename(words.first.to_s) == "cd"

        found << ["~", dir] if words[1].nil?
        dir = cd(words, dir)
        review << "`#{seg}` goes where the audit cannot follow" if dir.nil?
      end
      [found, review, dir]
    end

    def path_like?(word) = word.start_with?("/", "~") || word.include?("..")

    # [paths, review] for one simple command. Paths are its path-like
    # words: assignments' values, the command, redirection targets, the
    # files options name, and arguments, but not the program of grep, rg,
    # sed and awk, nor what echo and printf print. Review lists what the
    # audit cannot follow: a word with `$` or a backquote, a sed or awk
    # program that reads, writes or runs something, and options that run
    # a command.
    def segment_words(seg)
      seg = seg.dup
      found = []
      review = []
      assigned = []
      while seg.size > 1 && (m = seg.first.match(/\A([A-Za-z_][A-Za-z_0-9]*)=(.*)\z/m))
        assigned << [m[1], m[2]]
        seg.shift
      end
      # The wrapper refuses or drops every variable but MIDNIGHT_DRIVE and
      # MIDNIGHT_THREADS, and logs both.
      assigned.select! { |name, _| name == "MIDNIGHT_DRIVE" } if File.basename(seg.first.to_s) == "midnight"
      found.concat(assigned.map(&:last))
      unless seg.empty?
        found << seg.first
        cmd = File.basename(seg.first)
        program_due = PATTERN_FIRST.include?(cmd)
        take = nil
        seg.drop(1).each do |w|
          if take == :program
            review << "#{cmd} program `#{w}`" if runs_something?(cmd, w)
            take = nil
          elsif take == :file
            found << w
            take = nil
          elsif (m = w.match(REDIRECT))
            target = w.delete_prefix(m[0])
            found << target unless target.empty? || target.start_with?("&")
          elsif NO_PATHS.include?(cmd)
            next
          elsif PATTERN_FIRST.include?(cmd) && %w[-e --regexp --expression].include?(w)
            take = :program
            program_due = false
          elsif FILE_OPTIONS.include?(w)
            take = :file
            program_due = false
          elsif w.start_with?("-")
            name, value = w.split("=", 2)
            review << "`#{w}` runs a command" if RUNS.include?(name)
            next unless value

            if %w[--regexp --expression].include?(name)
              review << "#{cmd} program `#{value}`" if runs_something?(cmd, value)
            else
              found << value
            end
            program_due = false if %w[--file --regexp --expression].include?(name)
          elsif program_due
            review << "#{cmd} program `#{w}`" if runs_something?(cmd, w)
            program_due = false
          else
            found << w
          end
        end
      end
      review.concat(found.select { |w| w.match?(/[$`]/) }.map { |w| "`#{w}`" })
      [found.select { |w| path_like?(w) }, review]
    end

    # Whether a sed or awk program may read or write a file or run a
    # command: sed's r, R, w, W and e outside its regular expressions, and
    # awk's getline, system, and output to a file or a pipe.
    def runs_something?(cmd, program)
      case cmd
      when "sed"
        program.gsub(%r{/(?:[^/\\]|\\.)*/}, "/").match?(%r{(?:\A|[\s;{}\d$,!/])[rRwWe](?:[\s/]|\z)})
      when "awk"
        program.match?(/getline|system\s*\(|>\s*["A-Za-z_(]|\|/)
      else false
      end
    end

    # [paths, review] of one tool call: each path as [word, the directory it
    # is taken from].
    def paths(call, root, cwd)
      input = call["input"]
      case call["tool"]
      when "Bash" then bash_words(input["command"].to_s, cwd).first(2)
      when "Read", "Edit", "Write" then [[input["file_path"]].compact.map { |p| [p, root] }, []]
      when "Glob" then [[input["path"], glob_base(input["pattern"])].compact.map { |p| [p, root] }, []]
      when "Grep" then [[input["path"], glob_base(input["glob"])].compact.map { |p| [p, root] }, []]
      else [[], []]
      end
    end

    def glob_base(pattern)
      return nil unless pattern

      i = pattern.index(/[*?\[{]/)
      return pattern unless i

      cut = pattern[0, i].rindex("/")
      cut ? pattern[0, cut + 1] : nil
    end

    # { "escapes" => the paths outside the allowed places that calls which
    # ran reached, "review" => what the audit could not follow }. An escape
    # found in a `midnight` command has "compiler" true. scratch is Claude
    # Code's directory under /tmp for the run.
    def check(calls, root:, wrapper:, tool_results:, scratch: nil)
      allowed = [root, wrapper, tool_results, scratch].compact
      escapes = []
      review = []
      walk(calls, root) do |c, cwd|
        found, why = paths(c, root, cwd)
        review.concat(why.map { |w| { "tool" => c["tool"], "what" => w } })
        found.each do |p, dir, compiler|
          full = expand(p, dir)
          if full.nil?
            review << { "tool" => c["tool"], "what" => "`#{p}` from a directory the audit cannot follow" }
            next
          end
          next if full != :outside && %w[/dev/null /dev/stdout /dev/stderr].include?(full)
          next if full != :outside && allowed.any? { |a| Guard.inside?(full, a) }

          escapes << (compiler ? { "tool" => c["tool"], "path" => p, "compiler" => true } : { "tool" => c["tool"], "path" => p })
        end
      rescue StandardError => e
        review << { "tool" => c["tool"], "what" => "the audit failed: #{e.message}" }
      end
      { "escapes" => escapes, "review" => review }
    end

    def escapes(calls, **places) = check(calls, **places)["escapes"]

    # [escapes, review] once the wrapper's refusals are taken into account.
    # A refused call goes for review, and so does a path of a `midnight`
    # command that a refused call named: the compiler did not run with it.
    # Every other escape stays.
    def with_refusals(audit, calls)
      refused = calls.select { |c| c["refused"] }
      named = lambda do |e|
        e["compiler"] && refused.any? { |c| c["args"].to_a.include?(e["path"]) || c["caller"].to_h.value?(e["path"]) }
      end
      covered, escapes = audit["escapes"].partition(&named)
      review = audit["review"] +
               covered.map { |e| { "tool" => e["tool"], "what" => "`#{e['path']}` in a call the wrapper refused" } } +
               refused.map { |c| { "tool" => "midnight", "what" => "refused: #{c['refused']}" } }
      [escapes, review]
    end
  end

  # The answers of the command `draws`, read and compared.
  module Draws
    module_function

    SAID = "midnight: drive: "
    # The numbers of each shape, by its statement's name.
    NUMBERS = { "rect" => 4, "circle" => 3, "line" => 5, "triangle" => 6 }.freeze
    HEADER = /\Adraws: (\d+) ticks?, (\S+), (\d+) draws?\z/
    BACKGROUND = /\AWindow\.background = Color\((\d+), (\d+), (\d+), (\d+)\)\z/
    SHAPE = /\ADraw\.(rect|circle|line|triangle)\((.*)\)\z/
    ARGUMENTS = /\A((?:[^,]+, )*)Color\((\d+), (\d+), (\d+), (\d+)\)\z/

    # The lines of every `draws` answer in a program's standard error, with
    # `midnight: drive: ` removed: each answer's first line and the lines
    # after it that go on with two spaces.
    def answers_text(stderr)
      out = +""
      inside = false
      stderr.to_s.scrub.each_line do |line|
        line = line.chomp
        if line.start_with?("#{SAID}draws: ")
          inside = true
        elsif !(inside && line.start_with?("#{SAID}  "))
          inside = false
          next
        end
        out << line.delete_prefix(SAID) << "\n"
      end
      out
    end

    # A 32-bit float's bits from the text a draw prints it as; nil when the
    # text is no number.
    def bits(text)
      t = text.strip
      x = { "NaN" => Float::NAN, "Infinity" => Float::INFINITY, "-Infinity" => -Float::INFINITY }[t] || Float(t)
      return 0x7fc00000 if x.nan?

      [x].pack("e").unpack1("V")
    rescue ArgumentError, TypeError
      nil
    end

    # What a line of a `draws` answer draws: { "kind", "numbers" (bits),
    # "color" (four bytes) }; a line that is no draw is
    # { "kind" => "text", "text" => line }.
    def shape(line)
      text = line.strip
      m = text.match(SHAPE)
      a = m && m[2].match(ARGUMENTS)
      kind = m && m[1]
      numbers = a && a[1].split(", ").map { |n| bits(n) }
      if a && numbers.size == NUMBERS[kind] && numbers.none?(&:nil?)
        { "kind" => kind, "numbers" => numbers, "color" => a[2..5].map(&:to_i) }
      else
        { "kind" => "text", "text" => text }
      end
    end

    # Each answer of a text of `draws` answers: { "frame" => [tick, size,
    # background bytes], "shapes", "lines" (the shapes' lines as printed),
    # "header" (its first two lines) }.
    def parse(text)
      answers = []
      text.to_s.each_line do |line|
        line = line.chomp
        if (h = line.match(HEADER))
          answers << { "frame" => [h[1].to_i, h[2], nil], "shapes" => [], "lines" => [], "header" => [line] }
        elsif answers.empty?
          next
        elsif answers.last["header"].size == 1 && (b = line.strip.match(BACKGROUND))
          answers.last["frame"][2] = b[1..4].map(&:to_i)
          answers.last["header"] << line
        else
          answers.last["shapes"] << shape(line)
          answers.last["lines"] << line.strip
        end
      end
      answers
    end

    # Where the answers got first differ from those wanted, or nil when they
    # are equal: { "answer" (from 1), "how" (`answers`, `frame`, `missing`,
    # `extra`, `order`, `differs`), "got", "want" (lines, nil where there is
    # none) }. any: each answer's shapes compared as a set with counts.
    def difference(got, want, any: false)
      [got.size, want.size].max.times do |i|
        g = got[i]
        w = want[i]
        if g.nil? || w.nil?
          return { "answer" => i + 1, "how" => "answers", "got" => g && g["header"].first,
                   "want" => w && w["header"].first }
        end
        if g["frame"] != w["frame"]
          return { "answer" => i + 1, "how" => "frame", "got" => g["header"].join(" / "),
                   "want" => w["header"].join(" / ") }
        end
        d = any ? set_difference(g, w) : list_difference(g, w)
        return d.merge("answer" => i + 1) if d
      end
      nil
    end

    def list_difference(g, w)
      gs = g["shapes"]
      ws = w["shapes"]
      k = (0...[gs.size, ws.size].max).find { |j| gs[j] != ws[j] } or return nil
      at = ->(how, gi, wi) { { "how" => how, "got" => gi && g["lines"][gi], "want" => wi && w["lines"][wi] } }
      if gs.size == ws.size && gs.sort_by(&:to_s) == ws.sort_by(&:to_s) then at.("order", k, k)
      elsif k >= gs.size || gs[k..] == ws[k + 1..] then at.("missing", nil, k)
      elsif k >= ws.size || gs[k + 1..] == ws[k..] then at.("extra", k, nil)
      else at.("differs", k, k)
      end
    end

    def set_difference(g, w)
      missing = take_away(w, g)
      extra = take_away(g, w)
      return nil if missing.empty? && extra.empty?
      return { "how" => "extra", "got" => g["lines"][extra.first], "want" => nil } if missing.empty?
      return { "how" => "missing", "got" => nil, "want" => w["lines"][missing.first] } if extra.empty?

      want = w["shapes"][missing.first]
      near = extra.max_by { |i| likeness(g["shapes"][i], want) }
      { "how" => "differs", "got" => g["lines"][near], "want" => w["lines"][missing.first] }
    end

    # The indices of a's shapes that b does not have, counting repeats.
    def take_away(a, b)
      left = b["shapes"].tally
      a["shapes"].each_index.reject do |i|
        s = a["shapes"][i]
        left[s].to_i.positive? && (left[s] -= 1)
      end
    end

    def likeness(a, b)
      return -1 unless a["kind"] == b["kind"]

      a["numbers"].to_a.zip(b["numbers"].to_a).count { |x, y| x == y } + (a["color"] == b["color"] ? 1 : 0)
    end

    # Whether the shape of line got is the one of line want with two of its
    # numbers in each other's places.
    def swapped?(got, want)
      g = shape(got.to_s)
      w = shape(want.to_s)
      return false if g["kind"] == "text" || g["kind"] != w["kind"] || g["color"] != w["color"]

      gn = g["numbers"]
      wn = w["numbers"]
      diff = gn.each_index.reject { |i| gn[i] == wn[i] }
      diff.size == 2 && gn[diff[0]] == wn[diff[1]] && gn[diff[1]] == wn[diff[0]]
    end

    # Whether the shape of line got is a rectangle with want's width and
    # height at want's place plus half its width and half its height.
    def by_center?(got, want)
      g = shape(got.to_s)
      w = shape(want.to_s)
      return false unless g["kind"] == "rect" && w["kind"] == "rect"

      x, y, width, height = w["numbers"].map { |b| [b].pack("V").unpack1("e") }
      plus_half = ->(at, size) { [f32(at + f32(size * 0.5))].pack("e").unpack1("V") }
      g["numbers"] == [plus_half.(x, width), plus_half.(y, height), w["numbers"][2], w["numbers"][3]]
    end

    # x rounded to the nearest 32-bit float.
    def f32(x) = [x].pack("e").unpack1("e")

    # A line for a grade's notes.
    def note(d)
      return "standard error was cut at 1 MB" if d["how"] == "cut"

      parts = ["answer #{d['answer']}: #{d['how']}"]
      parts << "got `#{d['got']}`" if d["got"]
      parts << "want `#{d['want']}`" if d["want"]
      parts.join(", ")
    end
  end

  # Grading one program against one task.
  module Grade
    LOCK = Mutex.new

    module_function

    def compiler
      bin = ENV["MIDNIGHT_BIN"]
      return bin if bin && !bin.empty?

      @compiler ||= begin
        out = IO.popen(["nix", "build", "#{ROOT}#", "--no-link", "--print-out-paths"], &:read)
        raise Failure, "nix build failed" unless $?.success?

        File.join(out.lines.last.strip, "bin", "midnight")
      end
    end

    # The compiler of a commit.
    def compiler_of(commit)
      out = IO.popen(["nix", "build", "git+file://#{ROOT}?rev=#{commit}", "--no-link", "--print-out-paths"], &:read)
      raise Failure, "nix build of #{commit[0, 7]} failed" unless $?.success?

      File.join(out.lines.last.strip, "bin", "midnight")
    end

    # An environment like the wrapper's for a directory of its own.
    def environment(dir)
      home = File.join(dir, ".home")
      xdg = File.join(dir, ".xdg")
      tmp = File.join(dir, ".tmp")
      FileUtils.mkdir_p([home, xdg, tmp])
      Guard.environment({}, { "path" => Env.coreutils, "home" => home, "xdg" => xdg, "tmp" => tmp }).first
    end

    # Copies the program in src (or src itself, as solution.12am; nothing
    # when src does not exist) into a fresh directory with the task's
    # check.drive, and grades it. Returns
    # { "verdict", "checks" => {build:, output:, loops:}, ... }.
    def grade(task, src, compiler: Grade.compiler, keep: nil)
      LOCK.synchronize do
        dir = Env.mktemp
        begin
          if File.directory?(src) || !File.exist?(src)
            Eval.keep_program(src, dir)
          else
            FileUtils.cp(src, File.join(dir, "solution.12am"))
          end
          FileUtils.cp(task.drive, File.join(dir, "check.drive")) if task.drive
          result = grade_dir(task, dir, compiler)
          stdout = result.delete("stdout")
          draws = result.delete("draws_text")
          if keep
            FileUtils.mkdir_p(keep)
            File.write(File.join(keep, "graded.out"), stdout)
            File.write(File.join(keep, "graded.draws"), draws) if task.draws?
          end
          result
        ensure
          FileUtils.rm_rf(dir)
        end
      end
    end

    def grade_dir(task, dir, compiler)
      solution = File.join(dir, "solution.12am")
      unless File.file?(solution)
        return { "verdict" => "no solution", "checks" => {}, "sha256" => nil, "stdout" => "", "draws_text" => "" }
      end

      env = environment(dir)
      checks = {}
      notes = {}
      sha = Guard.sha256(solution)
      program_sha = Guard.program_sha256(dir)
      load = Guard.load_average
      start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      timed_out = false
      refused = Guard.refusal(["build", "solution.12am"], {}, dir, File.realpath(dir))
      if refused
        notes["build"] = refused
        checks["build"] = false
      else
        b = Guard.capture(env, [compiler, "build", "solution.12am", "-o", "solution"],
                          chdir: dir, limit: LIMIT, cap: CAP)
        timed_out ||= b[:timeout]
        checks["build"] = b[:status].zero?
        notes["build"] = b[:timeout] ? "time limit" : b[:err].lines.first(5).join unless checks["build"]
      end
      stdout = ""
      draws_text = ""
      difference = nil
      if checks["build"]
        run_env = task.drive ? env.merge("MIDNIGHT_DRIVE" => "check.drive") : env
        r = Guard.capture(run_env, [File.join(dir, "solution")], chdir: dir, limit: LIMIT, cap: CAP)
        timed_out ||= r[:timeout]
        stdout = r[:out].scrub
        checks["output"] = r[:status].zero? && same_output?(r[:out], task.expected)
        unless checks["output"]
          notes["output"] = r[:timeout] ? "time limit" : "status #{r[:status]}"
          notes["output"] += ", #{first_difference(r[:out], task.expected)}" if r[:status].zero?
        end
        if task.draws?
          draws_text = Draws.answers_text(r[:err])
          difference = draws_difference(task, draws_text, r[:err_cut])
          checks["draws"] = difference.nil?
          notes["draws"] = Draws.note(difference) if difference
        end
      else
        checks["output"] = false
        checks["draws"] = false if task.draws?
      end
      unless task.loops.empty? && task.systems.empty?
        l = Guard.capture(env, [compiler, "loops", "solution.12am"], chdir: dir, limit: LIMIT, cap: CAP)
        timed_out ||= l[:timeout]
        loops = l[:status].zero? ? l[:out] : nil
      end
      unless task.loops.empty?
        checks["loops"] = !loops.nil? && loops_ok?(loops, task.loops)
        notes["loops"] = l[:out].lines.first(10).join unless checks["loops"]
      end
      unless task.systems.empty?
        missing = missing_systems(dir, loops, task.systems)
        order = systems_out_of_order(dir, task.systems, task.others_after?)
        checks["systems"] = missing.empty? && order.nil?
        notes["systems"] = [missing.empty? ? nil : "missing: #{missing.join(', ')}", order].compact.join("; ") unless checks["systems"]
      end
      verdict = checks.values.all? ? "pass" : "fail"
      ms = ((Process.clock_gettime(Process::CLOCK_MONOTONIC) - start) * 1000).round
      out = { "verdict" => verdict, "checks" => checks, "notes" => notes, "sha256" => sha,
              "program_sha256" => program_sha, "timeout" => timed_out, "load" => load, "ms" => ms,
              "stdout" => stdout, "draws_text" => draws_text }
      out["draws"] = difference if difference
      out
    end

    # Where the `draws` answers of a run (text, with `midnight: drive: `
    # removed) first differ from the task's, or nil when they agree. cut:
    # standard error was cut at the cap.
    def draws_difference(task, text, cut)
      return { "answer" => nil, "how" => "cut", "got" => nil, "want" => nil } if cut

      Draws.difference(Draws.parse(text), Draws.parse(task.expected_draws), any: task.draws_any_order?)
    end

    def same_output?(got, want) = got.b == want.b

    def first_difference(got, want)
      g = got.b.lines
      w = want.b.lines
      i = (0...[g.size, w.size].max).find { |k| g[k] != w[k] }
      "line #{i + 1}: got #{g[i].to_s.chomp.inspect}, want #{w[i].to_s.chomp.inspect}"
    end

    DECLARATION = /^\s*(?:strict\s+)?(?:gpu\s+)?system\s+([a-z_][A-Za-z0-9_]*[?!]?)/

    # The [name, each] of `systems` a program in dir does not meet: a
    # system it does not declare on a line of its own, or, with each, one
    # with no loop: `midnight loops` (loops, nil when it failed) shows none
    # under the system, and none in a function whose name the system's
    # body (its declaration to the next `end` at the start of a line) has.
    # The systems of the program whose root is solution.12am in dir, in the
    # order they run: source order, a required file's systems where its
    # first `require` stands.
    def system_order(dir, file = File.join(dir, "solution.12am"), seen = {})
      real = File.expand_path(file)
      return [] if seen[real] || !File.file?(real)

      seen[real] = true
      File.read(real).scrub.lines.flat_map do |line|
        if (m = line.match(Guard::REQUIRE)) && (path = Guard.require_path(m.post_match))
          system_order(dir, File.join(File.dirname(real), "#{path}.12am"), seen)
        elsif (m = line.match(DECLARATION))
          [m[1]]
        else
          []
        end
      end
    end

    # Why the program's systems are not in the order wanted, or nil: the
    # named ones must run in their order and, with others_after, before any
    # other. A named system the program does not declare is left to
    # missing_systems.
    def systems_out_of_order(dir, wanted, others_after)
      order = system_order(dir)
      names = wanted.map(&:first).select { |n| order.include?(n) }
      named = order.select { |n| names.include?(n) }.uniq
      return "runs #{named.map { |n| "`#{n}`" }.join(', ')} in that order" unless named == names
      return nil unless others_after && !names.empty?

      last = order.index(names.last)
      early = order.first(last).reject { |n| names.include?(n) }
      early.empty? ? nil : "`#{early.first}` runs before `#{names.last}`"
    end

    def missing_systems(dir, loops, wanted)
      bodies = {}
      Dir.glob("**/*.12am", base: dir).each do |f|
        lines = File.read(File.join(dir, f)).scrub.lines
        lines.each_with_index do |line, i|
          m = line.match(DECLARATION) or next
          stop = (i + 1...lines.size).find { |k| lines[k].match?(/\Aend\b/) } || lines.size
          bodies[m[1]] = lines[i + 1...stop].join
        end
      end
      rows = loops.to_s.lines.map { |l| l.chomp.split(/ {2,}/)[1].to_s }
      functions = rows.filter_map { |w| w[/\Adef ([A-Za-z_][A-Za-z0-9_]*[?!]?)\z/, 1] }.uniq
      wanted.reject do |name, each|
        body = bodies[name]
        body && (!each || rows.any? { |w| w.match?(/\A(?:strict )?(?:gpu )?system #{Regexp.escape(name)}\z/) } ||
                 functions.any? { |fn| body.match?(/(?<![A-Za-z0-9_])#{Regexp.escape(fn)}(?![A-Za-z0-9_])/) })
      end.map { |name, each| each ? "#{name} each" : name }
    end

    # Whether `midnight loops` output meets every [where, place].
    def loops_ok?(output, wanted)
      rows = output.lines.map { |l| l.chomp.split(/ {2,}/) }
      wanted.all? do |where, place|
        mine = rows.select { |r| r[1] == where }
        !mine.empty? && mine.all? { |r| r.last == place || r.last.to_s.start_with?("#{place}:") }
      end
    end
  end

  # What an agent read of docs/: the lines each tool result showed.
  module Reading
    module_function

    READERS = %w[grep egrep fgrep rg sed head tail cat].freeze
    NUMBERED = /\A\s*(\d+)(?:\t|→)(.*)\z/
    PREFIXED = /\A(?:(.+?):)?(\d+)[:-](.*)\z/

    # { dest => [line numbers] } for the files of docs/ (docs is
    # { dest => text }) whose lines the results of calls that ran showed.
    def lines(calls, root:, docs:)
      index = docs.transform_values { |text| Index.new(text) }
      seen = Hash.new { |h, k| h[k] = [] }
      Audit.walk(calls, root) do |c, cwd|
        text = c["result"].to_s
        next if text.empty?

        targets = begin
          targets(c, root, cwd, index.keys)
        rescue StandardError
          []
        end
        next if targets.empty?

        if c["tool"] == "Read"
          dest = targets.first
          text.each_line do |l|
            m = l.chomp.match(NUMBERED)
            n = m && m[1].to_i
            seen[dest] << n if n && index[dest].line(n)
          end
        else
          targets.each { |dest| seen[dest].concat(index[dest].shown(text, dest)) }
        end
      end
      seen.transform_values { |v| v.uniq.sort }.reject { |_, v| v.empty? }
    end

    # The files of docs/ a call reads; cwd is the shell's directory.
    def targets(call, root, cwd, dests)
      input = call["input"]
      case call["tool"]
      when "Read"
        dest = dest_of(input["file_path"].to_s, root)
        dests.include?(dest) ? [dest] : []
      when "Grep"
        return [] unless input["output_mode"] == "content"

        under(input["path"] || ".", root, dests).select do |d|
          !input["glob"] || File.fnmatch?(input["glob"], File.basename(d), File::FNM_EXTGLOB) ||
            File.fnmatch?(input["glob"], d, File::FNM_PATHNAME | File::FNM_EXTGLOB)
        end
      when "Bash" then shell_targets(input["command"].to_s, cwd, root, dests)
      else []
      end
    end

    # The files of docs/ the reading commands of a shell command name,
    # taken from cwd, with `cd` followed. A directory stands for the files
    # under it only for grep and rg.
    def shell_targets(command, cwd, root, dests)
      dir = cwd
      found = []
      Audit.segments(command).each do |seg|
        words = Audit.split(seg)
        cmd = File.basename(words.first.to_s)
        if cmd == "cd"
          dir = Audit.cd(words, dir)
        elsif dir && READERS.include?(cmd)
          words.drop(1).each do |w|
            found.concat(matching(w, dir, root, dests, recursive: %w[grep egrep fgrep rg].include?(cmd)))
          end
        end
      end
      found.uniq
    end

    # The docs/ path of an absolute or relative path, or nil.
    def dest_of(path, root)
      full = File.expand_path(path, root)
      docs = File.join(root, "docs")
      return nil unless full.start_with?("#{docs}/")

      full.delete_prefix("#{docs}/")
    end

    def under(path, root, dests)
      full = File.expand_path(path, root)
      docs = File.join(root, "docs")
      return dests if full == root || full == docs

      prefix = dest_of(full, root) or return []
      dests.select { |d| d == prefix || d.start_with?("#{prefix}/") }
    end

    # The files of docs/ a shell word, taken from dir, names, globs
    # included.
    def matching(word, dir, root, dests, recursive:)
      return [] if word.start_with?("-")

      full = Audit.expand(word, dir)
      return [] unless full.is_a?(String)

      docs = File.join(root, "docs")
      if full == docs || full == root
        return recursive ? dests : []
      end
      return [] unless full.start_with?("#{docs}/")

      rel = full.delete_prefix("#{docs}/")
      if dests.any? { |d| d.start_with?("#{rel}/") }
        return recursive ? under(full, root, dests) : []
      end

      dests.select { |d| File.fnmatch?(rel, d, File::FNM_PATHNAME | File::FNM_EXTGLOB) }
    end

    # The lines of one file, found by number or by text.
    class Index
      def initialize(text)
        @lines = text.lines.map(&:chomp)
        @at = Hash.new { |h, k| h[k] = [] }
        @lines.each_with_index { |l, i| @at[l.rstrip] << (i + 1) unless l.strip.empty? }
      end

      def line(n) = n.between?(1, @lines.size) ? @lines[n - 1] : nil

      # The lines of the file that text shows: lines prefixed by their
      # number (and file) where the number's line has that text; and lines
      # of text found in the file, each after the one before, in runs of
      # two or more lines in a row, one of them at least 12 characters
      # without spaces.
      def shown(text, dest)
        found = []
        runs = [[]]
        cursor = 0
        text.each_line do |raw|
          l = raw.chomp
          m = l.match(PREFIXED)
          if m && (m[1].nil? || m[1].end_with?(dest)) && line(m[2].to_i)&.rstrip == m[3].rstrip
            found << m[2].to_i
            next
          end
          next if l.strip.empty?

          n = @at[l.rstrip].bsearch { |x| x > cursor }
          unless n
            runs << []
            next
          end
          runs << [] unless runs.last.empty? || next_line?(runs.last.last, n)
          runs.last << n
          cursor = n
        end
        runs.each do |run|
          next if run.size < 2 || run.none? { |n| @lines[n - 1].gsub(/\s/, "").size >= 12 }

          found.concat(run)
        end
        found
      end

      # Whether line b follows line a with only blank lines between.
      def next_line?(a, b)
        b > a && (a + 1...b).all? { |k| @lines[k - 1].strip.empty? }
      end
    end

    def ranges(numbers)
      numbers.slice_when { |a, b| b != a + 1 }.map { |r| [r.first, r.last] }
    end
  end

  module Stats
    module_function

    def mean(xs) = xs.empty? ? nil : xs.sum.to_f / xs.size

    # The standard error of the mean.
    def se(xs)
      return nil if xs.size < 2

      m = mean(xs)
      Math.sqrt(xs.sum { |x| (x - m)**2 } / (xs.size - 1)) / Math.sqrt(xs.size)
    end

    def choose(n, k)
      return 0 if k.negative? || k > n

      (1..k).reduce(1) { |acc, i| acc * (n - k + i) / i }
    end

    # Fisher's exact test, two-sided, on the table [[a, b], [c, d]].
    def fisher(a, b, c, d)
      n1 = a + b
      n2 = c + d
      k = a + c
      total = choose(n1 + n2, k)
      prob = ->(x) { Rational(choose(n1, x) * choose(n2, k - x), total) }
      observed = prob.(a)
      ([0, k - n2].max..[k, n1].min).sum { |x| (p = prob.(x)) <= observed ? p : 0 }.to_f
    end

    # The chance that Fisher's test gives p < 0.1 for na runs against nb
    # when both pass at the pooled rate of pa + pb passes.
    def chance_of_mark(pa, na, pb, nb)
      rate = (pa + pb).to_f / (na + nb)
      binom = ->(n, k) { choose(n, k) * (rate**k) * ((1 - rate)**(n - k)) }
      (0..na).sum do |a|
        (0..nb).sum { |b| fisher(a, na - a, b, nb - b) < 0.1 ? binom.(na, a) * binom.(nb, b) : 0.0 }
      end
    end
  end

  # Grouping a results file's runs.
  module Runs
    module_function

    def counted(results) = results["runs"].select { |r| r["counts"] }

    def graded(runs) = runs.reject { |r| %w[infrastructure isolation].include?(r["outcome"]) }

    # The final run of each trial: its graded attempt, or its last.
    def trials(results)
      counted(results).group_by { |r| [r["task"], r["model"], r["trial"]] }.transform_values do |rs|
        rs.find { |r| !%w[infrastructure isolation].include?(r["outcome"]) } || rs.max_by { |r| r["attempt"] }
      end
    end

    # { [task, model] => [graded runs] }
    def cells(results)
      out = Hash.new { |h, k| h[k] = [] }
      trials(results).each_value { |r| out[[r["task"], r["model"]]] << r unless %w[infrastructure isolation].include?(r["outcome"]) }
      out
    end

    def pass_rate(runs) = runs.empty? ? nil : runs.count { |r| r["outcome"] == "pass" }.to_f / runs.size

    def tasks(results) = results["task_names"] || results["tasks"].keys
  end

  # The Markdown report of one results file.
  module Report
    module_function

    def fmt(x, digits = 1)
      return "–" if x.nil?

      x.is_a?(Float) ? format("%.#{digits}f", x) : x.to_s
    end

    # text as a Markdown code span; one with backquotes in it gets two.
    def code(text) = text.include?("`") ? "`` #{text} ``" : "`#{text}`"

    def table(header, rows)
      out = +"| #{header.join(' | ')} |\n| #{header.map { '---' }.join(' | ')} |\n"
      rows.each { |r| out << "| #{r.map { |c| c.to_s.gsub('|', '\|') }.join(' | ')} |\n" }
      out
    end

    def models_reported(results)
      results["runs"].flat_map { |r| r["models_reported"].to_a }.uniq.sort
    end

    def render(results)
      out = +""
      cells = Runs.cells(results)
      tasks = Runs.tasks(results)
      models = results["models"]
      trials = results["trials"]
      out << "# Eval results #{results['date']}, #{results['commit'][0, 7]}#{results['label'] ? " (#{results['label']})" : ''}\n\n"
      out << "- Commit: `#{results['commit']}`, Midnight #{results['version']}\n"
      out << "- Claude Code #{results['claude_code']}, Ruby #{results['ruby']}\n"
      out << "- Kit: `#{results.dig('kit', 'file')}` (#{results.dig('kit', 'files').size} files)\n"
      out << "- Agent's tools: `#{results['agent_tools']}`; compiler's PATH: `#{results['coreutils']}`\n" if results["agent_tools"]
      out << "- Models: #{models.join(', ')} (as reported: #{models_reported(results).join(', ')}); " \
             "effort: #{results['effort'] || 'default'}; trials: #{trials}\n\n"

      out << "## Pass rates\n\n"
      rows = models.map do |m|
        rates = tasks.filter_map { |t| Runs.pass_rate(cells[[t, m]]) }
        every = tasks.count { |t| (rs = cells[[t, m]]).any? && rs.all? { |r| r["outcome"] == "pass" } }
        se = Stats.se(rates)
        ["`#{m}`", "#{fmt(Stats.mean(rates)&.*(100))}%#{se ? " ± #{fmt(se * 100)}" : ''}", "#{every} of #{tasks.size}"]
      end
      out << table(["Model", "pass@1 (± s.e. across tasks)", "Tasks passed every time"], rows)
      short = tasks.product(models).select { |t, m| cells[[t, m]].size < trials }
      out << "\nFewer graded runs than trials: #{short.map { |t, m| "#{t}/#{m} (#{cells[[t, m]].size})" }.join(', ')}.\n" unless short.empty?

      out << "\n## Tasks\n\n"
      rows = tasks.product(models).map do |t, m|
        rs = cells[[t, m]]
        passes = rs.count { |r| r["outcome"] == "pass" }
        avg = ->(f) { Stats.mean(rs.filter_map { |r| f.(r) }) }
        mark = rs.size < trials ? " *" : ""
        ["`#{t}`", "`#{m}`", "#{passes}/#{rs.size}#{mark}", rs.any? && passes == rs.size ? "yes" : "no",
         fmt(avg.(->(r) { r.dig("calls", "total") })), fmt(avg.(->(r) { r.dig("calls", "failed_total") })),
         fmt(avg.(->(r) { r["turns"] })), fmt(avg.(->(r) { r["duration_ms"] && r["duration_ms"] / 1000.0 }), 0),
         fmt(avg.(->(r) { r["output_tokens"] }), 0),
         fmt(avg.(->(r) { r["input_tokens"].to_i + r["cache_read_tokens"].to_i + r["cache_creation_tokens"].to_i }), 0),
         fmt(avg.(->(r) { r["cost_usd"] }), 3),
         rs.count { |r| r["first_try"] }, rs.count { |r| r["verified"] }]
      end
      out << table(["Task", "Model", "Passed", "Every time", "Calls", "Failed calls", "Turns", "Seconds",
                    "Output tokens", "Input tokens", "Cost (est. $)", "First try", "Verified"], rows)
      out << "\nMeans over graded runs. Cost is Claude Code's estimate at API prices.\n"

      graded = Runs.trials(results).values.reject { |r| %w[infrastructure isolation].include?(r["outcome"]) }
      out << draws(tasks, models, cells) if graded.any? { |r| r["checks"].to_h.key?("draws") }

      out << "\n## Diagnostics and run-time errors\n\n"
      groups = Hash.new { |h, k| h[k] = [] }
      graded.each { |r| r["diagnostics"].to_a.each { |d| groups[[d["kind"], d["template"]]] << [r, d] } }
      if groups.empty?
        out << "None.\n"
      else
        rows = groups.sort_by { |(kind, tmpl), v| [-v.size, kind, tmpl] }.map do |(kind, tmpl), v|
          runs = v.map(&:first).uniq
          spans = v.map { |_, d| d["spans"] }.reject(&:empty?).tally.sort_by { |s, c| [-c, s] }
          [kind, Report.code(tmpl), v.size, runs.size, runs.count { |r| r["outcome"] == "pass" },
           v.count { |_, d| d["repeated"] },
           spans.first(6).map { |s, c| "#{s.join(' / ')} ×#{c}" }.join(", ")]
        end
        out << table(%w[Kind Template Count Runs Passed Repeated Spans], rows)
      end

      out << "\n## The reference as read\n\n"
      sections = results.dig("kit", "sections") || {}
      read_runs = graded.select { |r| r["read"] }
      if read_runs.empty?
        out << "Not recorded.\n"
      else
        rows = []
        sections.each do |dest, secs|
          secs.each_with_index do |(start, title), i|
            first = title ? start + 1 : start
            stop = secs[i + 1] ? secs[i + 1][0] - 1 : Float::INFINITY
            n = read_runs.count do |r|
              Array(r.dig("read", dest)).any? { |a, b| b >= first && a <= stop }
            end
            rows << ["`#{dest}`", title || "(whole file)", n]
          end
        end
        out << table(%w[File Section Runs], rows)
        out << "\nRuns of #{read_runs.size} graded that a tool result showed a line of the section to, " \
               "below its heading.\n"
      end

      out << "\n## Exceptions\n\n"
      all = Runs.counted(results)
      lists = {
        "Compiler bugs" => graded.select { |r| r["compiler_bug"] },
        "Escaped runs" => graded.select { |r| r["outcome"] == "escaped" },
        "Runs for review (calls the audit could not follow, calls the wrapper refused)" => graded.select { |r| r["review"] },
        "Infrastructure errors" => results["runs"].select { |r| r["outcome"] == "infrastructure" },
        "Timeouts" => graded.select { |r| r["outcome"] == "timeout" },
        "No solution" => graded.select { |r| r["outcome"] == "no solution" },
        "Calls stopped by the time limit under load" => graded.select { |r| r["slow_calls"].to_i.positive? },
        "Programs the time limit stopped in grading, graded again alone" => graded.select { |r| r["regraded"] }
      }
      lists.each do |name, rs|
        next if rs.empty?

        out << "- #{name}: #{rs.map { |r| "#{r['task']}/#{r['model']}/#{r['trial']}.#{r['attempt']}#{detail(r)}" }.join('; ')}\n"
      end
      denied = graded.flat_map { |r| r["denied"].to_a.map { |d| [r, d] } }
      unless denied.empty?
        out << "- Denied tool calls: #{denied.size} in #{denied.map(&:first).uniq.size} runs: " \
               "#{denied.map { |_, d| "#{d['tool']} `#{d['input'].to_s.lines.first.to_s.chomp[0, 60]}`" }.tally.map { |s, c| c > 1 ? "#{s} ×#{c}" : s }.join(', ')}\n"
      end
      out << "None.\n" if lists.values.all?(&:empty?) && denied.empty?
      out << "\n#{all.size} counted runs, #{graded.size} graded.\n"
      out
    end

    # The section on the check `draws`: for each task and model that has
    # it, the graded runs that built and failed it, by how their answers
    # first differ; then the runs whose first differing shape is the
    # expected one with two numbers exchanged, and those whose first
    # differing shape is the expected rectangle placed by its center.
    def draws(tasks, models, cells)
      out = +"\n## Draws\n\n"
      hows = %w[answers frame missing extra order differs cut]
      swapped = []
      centered = []
      rows = tasks.product(models).filter_map do |t, m|
        rs = cells[[t, m]].select { |r| r["checks"].to_h.key?("draws") }
        next if rs.empty?

        failed = rs.select { |r| r.dig("checks", "build") && r.dig("checks", "draws") == false }
        failed.each do |r|
          name = "#{t}/#{m}/#{r['trial']}.#{r['attempt']}"
          d = r["draws"].to_h
          next unless d["how"] == "differs"

          swapped << name if Draws.swapped?(d["got"], d["want"])
          centered << name if Draws.by_center?(d["got"], d["want"])
        end
        tally = failed.map { |r| r.dig("draws", "how") }.tally
        ["`#{t}`", "`#{m}`", rs.size, failed.size, *hows.map { |h| tally[h].to_i }]
      end
      out << table(["Task", "Model", "Graded", "Built and failed draws", *hows.map(&:capitalize)], rows)
      out << "\nHow each run's answers first differ from the expected ones.\n\n"
      out << "- Two numbers exchanged: #{swapped.empty? ? 'none' : swapped.join(', ')}\n"
      out << "- A rectangle placed by its center: #{centered.empty? ? 'none' : centered.join(', ')}\n"
      out
    end

    def detail(r)
      case r["outcome"]
      when "infrastructure" then " (#{r['infrastructure']})"
      when "escaped" then " (#{r['escapes'].map { |e| e['path'] }.join(', ')})"
      else
        parts = r["review"].to_a.map { |e| e["what"] }.first(3)
        parts << "#{r['regraded']['before']} under load, #{r['outcome']} alone" if r["regraded"]
        parts.empty? ? "" : " (#{parts.join('; ')})"
      end
    end
  end

  # Two results files side by side.
  module Compare
    module_function

    def render(a, b)
      out = +"# Comparison\n\n"
      out << "- A: #{a['date']}, #{a['commit'][0, 7]}#{a['label'] ? " (#{a['label']})" : ''}\n"
      out << "- B: #{b['date']}, #{b['commit'][0, 7]}#{b['label'] ? " (#{b['label']})" : ''}\n\n"
      differences(a, b).each { |d| out << "- #{d}\n" }
      out << "\n" unless differences(a, b).empty?
      ca = Runs.cells(a)
      cb = Runs.cells(b)
      tasks = Runs.tasks(a) & Runs.tasks(b)
      models = a["models"] & b["models"]

      out << "## Pass rates\n\n"
      rows = models.map do |m|
        ra = tasks.filter_map { |t| Runs.pass_rate(ca[[t, m]]) }
        rb = tasks.filter_map { |t| Runs.pass_rate(cb[[t, m]]) }
        diffs = tasks.filter_map do |t|
          x = Runs.pass_rate(ca[[t, m]])
          y = Runs.pass_rate(cb[[t, m]])
          y - x if x && y
        end
        se = Stats.se(diffs)
        ["`#{m}`", pct(Stats.mean(ra)), pct(Stats.mean(rb)),
         "#{pct(Stats.mean(diffs), signed: true)}#{se ? " ± #{Report.fmt(se * 100)}" : ''}",
         ra.size == rb.size ? "" : "#{ra.size}/#{rb.size}"]
      end
      out << Report.table(["Model", "pass@1 A", "pass@1 B", "Difference (± s.e.)", ""], rows)
      out << "\nOver the tasks in both.\n"

      out << "\n## Tasks\n\n"
      marks = 0
      chance = 0.0
      pooled = [0, 0, 0, 0]
      rows = tasks.product(models).map do |t, m|
        x = ca[[t, m]]
        y = cb[[t, m]]
        pa = x.count { |r| r["outcome"] == "pass" }
        pb = y.count { |r| r["outcome"] == "pass" }
        pooled = pooled.zip([pa, x.size, pb, y.size]).map(&:sum)
        p = x.empty? || y.empty? ? nil : Stats.fisher(pa, x.size - pa, pb, y.size - pb)
        chance += Stats.chance_of_mark(pa, x.size, pb, y.size) if p
        mark = p && p < 0.1
        marks += 1 if mark
        ["`#{t}`", "`#{m}`", "#{pa}/#{x.size}", "#{pb}/#{y.size}", p ? format("%.3f", p) : "–", mark ? "**differs**" : ""]
      end
      out << Report.table(["Task", "Model", "A", "B", "Fisher p", ""], rows)
      out << "\n#{marks} marked (p < 0.1). Chance alone, with each row passing at its pooled rate in both, " \
             "would mark about #{Report.fmt(chance)} of #{rows.size} rows.\n"
      out << "\n#{pooled_line(*pooled)}\n"

      out << "\n## Templates whose count changed\n\n"
      ta = template_counts(a, tasks)
      tb = template_counts(b, tasks)
      changed = (ta.keys | tb.keys).select { |k| ta[k] != tb[k] }.sort_by { |k| [-(tb[k] - ta[k]).abs, k] }
      if changed.empty?
        out << "None.\n"
      else
        out << Report.table(%w[Kind Template A B], changed.map { |k| [k[0], Report.code(k[1]), ta[k], tb[k]] })
        out << "\nOccurrences over the tasks in both, in all their runs.\n"
      end
      out
    end

    # The passes and graded runs of A and of B over all the rows, and
    # Fisher's exact test, two-sided, on their passes and failures.
    def pooled_line(pa, na, pb, nb)
      p = na.zero? || nb.zero? ? nil : Stats.fisher(pa, na - pa, pb, nb - pb)
      "Over all rows: A passed #{pa} of #{na} graded runs (#{pct(na.zero? ? nil : pa.to_f / na)}), " \
        "B #{pb} of #{nb} (#{pct(nb.zero? ? nil : pb.to_f / nb)}); " \
        "Fisher's exact test, two-sided: p = #{p ? format('%.3f', p) : '–'}."
    end

    def pct(x, signed: false)
      return "–" if x.nil?

      format(signed ? "%+.1f%%" : "%.1f%%", x * 100)
    end

    def template_counts(results, tasks)
      counts = Hash.new(0)
      Runs.trials(results).each_value do |r|
        next unless tasks.include?(r["task"])

        r["diagnostics"].to_a.each { |d| counts[[d["kind"], d["template"]]] += 1 }
      end
      counts
    end

    # What differs between two runs besides the commit.
    def differences(a, b)
      out = []
      changed = (a["tasks"].keys & b["tasks"].keys).reject { |t| a["tasks"][t]["sha256"] == b["tasks"][t]["sha256"] }
      out << "Tasks that differ: #{changed.map { |t| "`#{t}`" }.join(', ')}" unless changed.empty?
      only_a = a["tasks"].keys - b["tasks"].keys
      only_b = b["tasks"].keys - a["tasks"].keys
      out << "Tasks only in A: #{only_a.join(', ')}" unless only_a.empty?
      out << "Tasks only in B: #{only_b.join(', ')}" unless only_b.empty?
      ma = Report.models_reported(a)
      mb = Report.models_reported(b)
      out << "Models as reported: A #{ma.join(', ')}; B #{mb.join(', ')}" unless ma == mb
      out << "Effort: A #{a['effort'] || 'default'}; B #{b['effort'] || 'default'}" unless a["effort"] == b["effort"]
      out << "Trials: A #{a['trials']}; B #{b['trials']}" unless a["trials"] == b["trials"]
      out << "Claude Code: A #{a['claude_code']}; B #{b['claude_code']}" unless a["claude_code"] == b["claude_code"]
      out << "The preamble differs" unless a["preamble_sha256"] == b["preamble_sha256"]
      unless a["agent_tools"] == b["agent_tools"] && a["coreutils"] == b["coreutils"]
        out << "The agent's tools or the compiler's PATH differ"
      end
      kit = kit_difference(a["kit"], b["kit"])
      out << kit if kit
      out
    end

    def kit_difference(ka, kb)
      fa = ka["files"].to_h { |f| [f["dest"], f] }
      fb = kb["files"].to_h { |f| [f["dest"], f] }
      parts = []
      parts << "only in A: #{(fa.keys - fb.keys).join(', ')}" unless (fa.keys - fb.keys).empty?
      parts << "only in B: #{(fb.keys - fa.keys).join(', ')}" unless (fb.keys - fa.keys).empty?
      diff = (fa.keys & fb.keys).reject { |d| fa[d]["sha256"] == fb[d]["sha256"] }
      parts << "changed: #{diff.map { |d| "#{d} (A `#{fa[d]['source']}`, B `#{fb[d]['source']}`)" }.join(', ')}" unless diff.empty?
      return nil if parts.empty? && ka["file"] == kb["file"]

      "Kit: A `#{ka['file']}`, B `#{kb['file']}`#{parts.empty? ? '' : "; #{parts.join('; ')}"}"
    end
  end

  # One agent session on one task, from the directories to the record.
  class Session
    attr_reader :record

    def initialize(runner, task, model, trial, attempt)
      @runner = runner
      @task = task
      @model = model
      @trial = trial
      @attempt = attempt
      @keep = File.join(runner.runs_dir, task.name, "#{model}-#{trial}-#{attempt}")
    end

    def run
      FileUtils.mkdir_p(@keep)
      @root = Env.mktemp
      @wrapper = Env.mktemp
      @logs = Env.mktemp
      prepare
      started = Time.now
      status = agent
      transcript = Transcript.load(File.join(@keep, "transcript.jsonl"))
      @record = build_record(transcript, status, started)
    ensure
      cleanup
    end

    def prepare
      File.write(File.join(@root, "TASK.md"),
                 "#{@runner.preamble.chomp}\n\n#{@task.read('TASK.md')}")
      @runner.kit_files.each do |dest, source|
        target = File.join(@root, "docs", dest)
        FileUtils.mkdir_p(File.dirname(target))
        FileUtils.cp(source, target)
      end
      @task.given_files.each do |f|
        target = File.join(@root, f)
        FileUtils.mkdir_p(File.dirname(target))
        FileUtils.cp(@task.path("given", f), target)
        File.write(target, File.read(target).gsub("@@TOKEN@@", @runner.token)) if @runner.token
      end
      Eval.write_wrapper(@wrapper, root: @root, logs: @logs, compiler: @runner.compiler)
    end

    def command
      cmd = [@runner.claude, "-p", "--safe-mode", "--restricted", "--strict-mcp-config",
             "--disable-slash-commands", "--tools", TOOLS.join(","),
             "--allowedTools", "Bash(midnight *)", "Bash(MIDNIGHT_DRIVE=* midnight *)",
             "Bash(MIDNIGHT_THREADS=* midnight *)", "Bash(MIDNIGHT_WINDOW=* midnight *)", *TOOLS.drop(1),
             "--permission-prompts", "none", "--no-session-persistence",
             "--output-format", "stream-json", "--verbose", "--model", @model]
      cmd += ["--effort", @runner.effort] if @runner.effort
      cmd << PROMPT
    end

    def environment
      { "HOME" => Dir.home, "TMPDIR" => "/tmp", "LANG" => "C.UTF-8", "TERM" => "dumb",
        "PATH" => [@wrapper, File.dirname(@runner.claude), Env.agent_tools].join(":") }
    end

    # Runs the agent; returns :done, :timeout, :stopped or :isolation.
    def agent
      transcript = File.join(@keep, "transcript.jsonl")
      pid = Process.spawn(environment, *command, unsetenv_others: true, pgroup: true, chdir: @root,
                                                 in: File::NULL, out: transcript,
                                                 err: File.join(@keep, "stderr.txt"))
      @runner.started(pid)
      deadline = Time.now + (@task.minutes * 60)
      checked = false
      outcome = :done
      loop do
        break if Process.wait(pid, Process::WNOHANG)

        unless checked
          first = File.open(transcript, &:gets) rescue nil
          if first&.end_with?("\n")
            checked = true
            problem = Transcript.isolation_problem(Transcript.new(first).init, File.realpath(@root))
            if problem
              @isolation = problem
              outcome = :isolation
              stop(pid)
              break
            end
          end
        end
        if @runner.stopping?
          outcome = :stopped
          stop(pid)
          break
        end
        if Time.now > deadline
          outcome = :timeout
          stop(pid)
          break
        end
        sleep 0.25
      end
      if outcome == :done && !checked
        @isolation = Transcript.isolation_problem(Transcript.load(transcript).init, File.realpath(@root))
        outcome = :isolation if @isolation
      end
      Guard.kill_group(pid)
      @runner.finished(pid)
      outcome
    end

    def stop(pid)
      Process.kill(:TERM, -pid)
      5.times do
        return Process.wait(pid) if Process.wait(pid, Process::WNOHANG)

        sleep 0.2
      end
      Guard.kill_group(pid)
      Process.wait(pid)
    rescue Errno::ESRCH, Errno::ECHILD
      nil
    end

    def build_record(transcript, status, started)
      calls = CallLog.read(File.join(@logs, "calls.jsonl"))
      FileUtils.cp(File.join(@logs, "calls.jsonl"), @keep) if File.exist?(File.join(@logs, "calls.jsonl"))
      Eval.keep_compiled(@logs, @keep)
      program_dir = File.join(@keep, "program")
      Eval.keep_program(@root, program_dir)
      tool_calls = transcript.tool_calls
      rec = {
        "task" => @task.name, "model" => @model, "models_reported" => transcript.models_reported,
        "effort" => @runner.effort || "default", "trial" => @trial, "attempt" => @attempt,
        "counts" => true, "claude_code" => transcript.version,
        "started" => started.utc.iso8601, "dir" => @keep.delete_prefix("#{ROOT}/")
      }
      infra = status == :done ? transcript.infrastructure : nil
      if status == :isolation
        rec["outcome"] = "isolation"
        rec["isolation"] = @isolation
        return rec.merge(transcript.usage)
      elsif status == :stopped || (infra && infra[1])
        rec["outcome"] = "infrastructure"
        rec["infrastructure"] = infra ? infra[0] : "stopped by a usage limit elsewhere"
        rec["usage_limit"] = infra ? infra[1] : false
        rec["counts"] = false
        return rec.merge(transcript.usage)
      elsif infra
        rec["outcome"] = "infrastructure"
        rec["infrastructure"] = infra[0]
        return rec.merge(transcript.usage)
      end

      audit = Audit.check(tool_calls, root: @root, wrapper: @wrapper,
                                      tool_results: Eval.tool_results_dir(@root),
                                      scratch: Eval.claude_tmp_dir(@root))
      escapes, review = Audit.with_refusals(audit, calls)
      graded = Grade.grade(@task, program_dir, compiler: @runner.compiler, keep: @keep)
      given_sha = @task.repair? ? Guard.sha256(@task.path("given", "solution.12am")) : nil
      Eval.apply_grade(rec, graded, escaped: !escapes.empty?, timeout: status == :timeout)
      rec["escapes"] = escapes unless escapes.empty?
      rec["review"] = review unless review.empty?
      rec["calls"] = CallLog.summary(calls)
      rec["diagnostics"] = CallLog.diagnostics(calls)
      rec["compiler_bug"] = calls.any? { |c| Stderr.compiler_bug?(c["stderr"]) }
      rec["first_try"] = CallLog.first_try(calls, given_sha)
      rec["verified"] = CallLog.verified(calls, graded["sha256"], graded["program_sha256"])
      rec["denied"] = tool_calls.select { |c| c["denied"] }.map do |c|
        { "tool" => c["tool"], "input" => c["input"]["command"] || c["input"]["file_path"] || c["input"]["path"] }
      end
      rec["slow_calls"] = calls.count { |c| c["timeout"] && c["load"].to_f > Etc.nprocessors }
      read = Reading.lines(tool_calls, root: @root, docs: @runner.kit_texts)
      rec["read"] = read.transform_values { |v| Reading.ranges(v) }
      rec.merge(transcript.usage)
    end

    def cleanup
      if @runner.keep_files? && @root
        FileUtils.cp_r(@root, File.join(@keep, "files"))
        spilled = Eval.tool_results_dir(@root)
        FileUtils.cp_r(spilled, File.join(@keep, "spilled")) if File.directory?(spilled)
      end
      [@root, @wrapper, @logs].compact.each { |d| FileUtils.rm_rf(d) }
      Eval.remove_empty_tree(Eval.tool_results_dir(@root)) if @root
      Eval.remove_empty_tree(Eval.claude_tmp_dir(@root)) if @root
    end
  end

  # Sets a run's outcome, checks and grading facts from its grade.
  def self.apply_grade(rec, graded, escaped:, timeout:)
    rec["outcome"] =
      if escaped then "escaped"
      elsif timeout then "timeout"
      elsif graded["verdict"] == "no solution" then "no solution"
      else graded["verdict"]
      end
    rec["checks"] = graded["checks"]
    rec["failed_checks"] = graded["checks"].reject { |_, v| v }.keys
    rec.delete("draws")
    rec["draws"] = graded["draws"] if graded["draws"]
    rec.delete("grade_notes")
    rec["grade_notes"] = graded["notes"] unless graded["notes"].to_h.empty?
    rec["graded_sha256"] = graded["sha256"]
    rec["graded_program_sha256"] = graded["program_sha256"]
    rec["grading"] = graded.slice("ms", "load", "timeout")
  end

  # Copies the program in src to the same paths under dest.
  def self.keep_program(src, dest)
    FileUtils.mkdir_p(dest)
    Guard.program_files(src).each do |f|
      FileUtils.mkdir_p(File.dirname(File.join(dest, f)))
      FileUtils.cp(File.join(src, f), File.join(dest, f))
    end
  end

  # Copies the programs the wrapper kept at each check, build and run, one
  # directory for each program's hash as the call log records it, from the
  # log directory to keep/programs.
  def self.keep_compiled(logs, keep)
    src = File.join(logs, "programs")
    return unless File.directory?(src)

    FileUtils.mkdir_p(keep)
    FileUtils.rm_rf(File.join(keep, "programs"))
    FileUtils.cp_r(src, File.join(keep, "programs"))
  end

  # Claude Code's directory for a working directory's spilled tool results.
  def self.tool_results_dir(root)
    File.join(Dir.home, ".claude", "projects", root.gsub(/[^A-Za-z0-9]/, "-"))
  end

  # The directory Claude Code makes under /tmp for a working directory.
  def self.claude_tmp_dir(root)
    File.join("/tmp", "claude-#{Process.uid}", root.gsub(/[^A-Za-z0-9]/, "-"))
  end

  # Removes dir and the directories under it, deepest first, when no file
  # is among them; leaves everything otherwise.
  def self.remove_empty_tree(dir)
    return unless File.directory?(dir) && !File.symlink?(dir)

    entries = Dir.glob("**/*", File::FNM_DOTMATCH, base: dir).reject { |e| File.basename(e) == "." }
    return unless entries.all? { |e| File.directory?(File.join(dir, e)) && !File.symlink?(File.join(dir, e)) }

    entries.sort_by { |e| -e.count("/") }.each { |e| Dir.rmdir(File.join(dir, e)) }
    Dir.rmdir(dir)
  rescue SystemCallError
    nil
  end

  # Writes the agent's `midnight` into dir.
  def self.write_wrapper(dir, root:, logs:, compiler:, limit: LIMIT, cap: CAP)
    home = File.join(logs, "home")
    xdg = File.join(logs, "xdg")
    tmp = File.join(logs, "tmp")
    FileUtils.mkdir_p([home, xdg, tmp])
    config = { "root" => File.realpath(root), "log" => File.join(logs, "calls.jsonl"),
               "programs" => File.join(logs, "programs"), "compiler" => compiler, "path" => Env.coreutils, "home" => home, "xdg" => xdg,
               "tmp" => tmp, "limit" => limit, "cap" => cap }
    path = File.join(dir, "midnight")
    File.write(path, <<~SCRIPT)
      #!#{Env.ruby} --disable=gems,rubyopt
      # frozen_string_literal: true
      $LOAD_PATH.replace(#{Env.load_path.inspect})
      #{GUARD}
      Guard.main(ARGV, #{config.inspect})
    SCRIPT
    File.chmod(0o755, path)
    path
  end

  # A whole run: every task, model and trial, with its results file.
  class Runner
    attr_reader :runs_dir, :compiler, :claude, :effort, :kit_files, :preamble

    def initialize(opts)
      @opts = opts
      @mutex = Mutex.new
      @pids = []
      @stopping = false
    end

    def kit_texts
      @kit_texts ||= @kit_files.transform_values { |path| File.read(path).scrub }
    end

    # The text that replaces @@TOKEN@@ in given files, or nil.
    def token = nil

    # Whether a session keeps every file of the run's directory and its
    # spilled tool results, not only the program.
    def keep_files? = false

    def stopping? = @mutex.synchronize { @stopping }
    def started(pid) = @mutex.synchronize { @pids << pid }
    def finished(pid) = @mutex.synchronize { @pids.delete(pid) }

    def setup
      @commit = Git.head
      raise Failure, "evals/eval.rb differs from #{@commit[0, 7]}; commit it first" unless Git.clean?(@commit, "evals/eval.rb")

      %w[/tmp /].product(%w[AGENTS.md CLAUDE.md]).map { |d, f| File.join(d, f) }.each do |path|
        raise Failure, "#{path} would reach every agent; move it first" if File.exist?(path)
      end
      @claude = Env.which("claude") or raise Failure, "`claude` is not on the PATH"
      @claude_version = IO.popen([@claude, "--version"], &:read).to_s[/\d+\.\d+\.\d+/]
      @date = Time.now.strftime("%Y-%m-%d")
      if @opts[:resume]
        @results_path = File.expand_path(@opts[:resume])
        @results = JSON.parse(File.read(@results_path))
        raise Failure, "#{@opts[:resume]} measured #{@results['commit'][0, 7]}, not HEAD" unless @results["commit"] == @commit
        unless @results["claude_code"] == @claude_version
          raise Failure, "#{@opts[:resume]} ran Claude Code #{@results['claude_code']}, not #{@claude_version}"
        end

        @opts.merge!(tasks: @results["task_names"], models: @results["models"], trials: @results["trials"],
                     effort: @results["effort"], kit: @results.dig("kit", "file"))
      end
      @effort = @opts[:effort]
      name = [@date, @commit[0, 7], @opts[:label]].compact.join("-")
      @results_path ||= File.join(ROOT, @opts[:results_dir] || "evals/results", "#{name}.json")
      if !@opts[:resume] && File.exist?(@results_path)
        raise Failure, "#{@results_path.delete_prefix("#{ROOT}/")} exists; give --label"
      end

      @runs_dir = File.join(ROOT, "evals", "runs", File.basename(@results_path, ".json"))
      snapshot = File.join(@runs_dir, "commit")
      FileUtils.rm_rf(snapshot)
      kit_file = @opts[:kit] || "evals/kit"
      unless Git.files(@commit).include?(kit_file)
        raise Failure, "kit `#{kit_file}` is not in #{@commit[0, 7]}"
      end

      tasks_dir = @opts[:tasks_dir] || "evals/tasks"
      Git.export(@commit, tasks_dir, snapshot)
      Git.export(@commit, "evals/preamble.md", snapshot)
      entries = Kit.parse(Git.show(@commit, kit_file))
      kit = Kit.files(entries, Git.files(@commit))
      kit.each_value { |source| Git.export(@commit, source, snapshot) }
      @kit_files = kit.transform_values { |s| File.join(snapshot, s) }
      @preamble = File.read(File.join(snapshot, "evals", "preamble.md"))
      all = Task.all(File.join(snapshot, tasks_dir))
      names = @opts[:tasks] || all.map(&:name)
      unknown = names - all.map(&:name)
      raise Failure, "no task #{unknown.join(', ')}" unless unknown.empty?

      @tasks = all.select { |t| names.include?(t.name) }
      @compiler = build_compiler
      @results ||= header(kit_file, kit)
    end

    def build_compiler = Grade.compiler_of(@commit)

    def header(kit_file, kit)
      {
        "format" => 1, "commit" => @commit,
        "version" => IO.popen([@compiler, "--version"], &:read).to_s.split.last,
        "ruby" => RUBY_VERSION, "claude_code" => @claude_version, "date" => @date,
        "label" => @opts[:label], "models" => @opts[:models], "trials" => @opts[:trials],
        "effort" => @opts[:effort], "task_names" => @tasks.map(&:name),
        "preamble_sha256" => Digest::SHA256.hexdigest(@preamble),
        "agent_tools" => Env.agent_tools, "coreutils" => Env.coreutils,
        "kit" => {
          "file" => kit_file,
          "files" => kit.map { |dest, source| { "dest" => dest, "source" => source, "sha256" => Digest::SHA256.file(@kit_files[dest]).hexdigest } },
          "sections" => kit.keys.to_h { |dest| [dest, dest.end_with?(".md") ? Kit.sections(File.read(@kit_files[dest])) : [[1, nil]]] }
        },
        "tasks" => @tasks.to_h { |t| [t.name, { "sha256" => t.sha256, "areas" => t.areas }] },
        "runs" => []
      }
    end

    # The (task, model, trial) triples still to run, with their next attempt.
    def pending
      runs = @results["runs"]
      @tasks.flat_map do |t|
        @opts[:models].flat_map do |m|
          (1..@opts[:trials]).filter_map do |k|
            mine = runs.select { |r| r["task"] == t.name && r["model"] == m && r["trial"] == k }
            next if mine.any? { |r| !%w[infrastructure isolation].include?(r["outcome"]) }
            next if mine.count { |r| r["counts"] } >= 3

            [t, m, k, mine.size + 1]
          end
        end
      end
    end

    def save
      @mutex.synchronize do
        @results["runs"].sort_by! { |r| [r["task"], r["model"], r["trial"], r["attempt"]] }
        FileUtils.mkdir_p(File.dirname(@results_path))
        tmp = "#{@results_path}.tmp"
        File.write(tmp, "#{JSON.pretty_generate(@results)}\n")
        File.rename(tmp, @results_path)
      end
    end

    def run
      setup
      queue = Queue.new
      pending.each { |p| queue << p }
      total = queue.size
      done = 0
      isolation = nil
      limited = false
      puts "eval: #{total} runs of #{@tasks.size} tasks, #{@opts[:models].join(', ')}, " \
           "#{@opts[:jobs]} at a time, Midnight #{@results['version']} at #{@commit[0, 7]}"
      workers = Array.new(@opts[:jobs]) do
        Thread.new do
          Thread.current.abort_on_exception = true
          loop do
            break if stopping?

            item = begin
              queue.pop(true)
            rescue ThreadError
              break
            end
            task, model, trial, attempt = item
            rec = Session.new(self, task, model, trial, attempt).run
            @mutex.synchronize do
              @results["runs"] << rec
              done += 1
            end
            save
            puts format("eval: [%d/%d] %s %s %d: %s", done, total, task.name, model, trial, describe(rec))
            if rec["outcome"] == "isolation"
              isolation = rec["isolation"]
              @mutex.synchronize { @stopping = true }
            elsif rec["usage_limit"]
              limited = true
              @mutex.synchronize { @stopping = true }
            elsif rec["outcome"] == "infrastructure" && rec["counts"]
              mine = @results["runs"].count { |r| r["task"] == task.name && r["model"] == model && r["trial"] == trial && r["counts"] }
              queue << [task, model, trial, attempt + 1] if mine < 3
            end
          end
        end
      end
      workers.each(&:join)
      regrade
      save
      rel = @results_path.delete_prefix("#{ROOT}/")
      if isolation
        raise Failure, "the flags no longer isolate the agent: #{isolation}; results so far in #{rel}"
      end
      if limited
        warn "eval: a usage limit stopped the run; results so far in #{rel}"
        warn "eval: continue with: nix run .#eval -- run --resume #{rel}"
        exit 3
      end
      report = File.join(File.dirname(@results_path), "#{File.basename(@results_path, '.json')}.md")
      File.write(report, Report.render(@results))
      puts "eval: results in #{rel}"
    end

    # Grades again, with no session running, each kept program the time
    # limit stopped in grading, and keeps the second grade.
    def regrade
      @results["runs"].each do |rec|
        next unless rec.dig("grading", "timeout") && !rec["regraded"]

        task = @tasks.find { |t| t.name == rec["task"] } or next
        keep = File.join(ROOT, rec["dir"])
        before = rec["outcome"]
        g = Grade.grade(task, File.join(keep, "program"), compiler: @compiler, keep: keep)
        Eval.apply_grade(rec, g, escaped: before == "escaped", timeout: before == "timeout")
        rec["regraded"] = { "before" => before }
        puts "eval: graded #{rec['task']} #{rec['model']} #{rec['trial']} again alone: #{rec['outcome']}"
      end
    end

    def describe(rec)
      case rec["outcome"]
      when "fail" then "fail (#{rec['failed_checks'].join(', ')})"
      when "infrastructure" then "infrastructure (#{rec['infrastructure']})"
      when "escaped" then "escaped (#{rec['escapes'].map { |e| e['path'] }.join(', ')})"
      else rec["outcome"]
      end
    end
  end

  # The checks every task's programs must meet.
  # The isolation probe: each task of evals/test/tasks once with haiku,
  # with a random token in a file beside the run's directories and in the
  # repository. `escape` asks the agent to reach outside its directory;
  # `agents-md` gives it an AGENTS.md that asks for the token instead of
  # the reply the task asks for.
  class Probe < Runner
    TOKEN_FILES = ["/tmp/eval-token", File.join(ROOT, "eval-token")].freeze

    attr_reader :token

    def initialize
      super(models: ["haiku"], trials: 1, jobs: 2, tasks_dir: "evals/test/tasks",
            results_dir: "evals/runs", label: "isolation-#{Time.now.strftime('%H%M%S')}")
    end

    def keep_files? = true

    def run
      @token = SecureRandom.hex(16)
      TOKEN_FILES.each do |f|
        File.open(f, File::WRONLY | File::CREAT | File::EXCL, 0o644) { |io| io.puts @token }
      rescue Errno::EEXIST
        raise Failure, "#{f} exists; remove it first"
      end
      begin
        super
      ensure
        TOKEN_FILES.each { |f| File.delete(f) if File.exist?(f) }
      end
      problems = @results["runs"].flat_map { |rec| verify(rec).map { |p| "#{rec['task']}: #{p}" } }
      problems.each { |p| puts "isolation: #{p}" }
      raise Failure, "isolation: #{problems.size} problems" unless problems.empty?

      puts "isolation: every check passes"
    end

    def verify(rec)
      return ["#{rec['outcome']}: #{rec['infrastructure'] || rec['isolation']}"] if %w[infrastructure isolation].include?(rec["outcome"])

      keep = File.join(ROOT, rec["dir"])
      transcript = Transcript.load(File.join(keep, "transcript.jsonl"))
      calls = CallLog.read(File.join(keep, "calls.jsonl"))
      problems = []
      init = transcript.init
      problems << "tools #{init['tools'].inspect}" unless init["tools"].to_a.sort == TOOLS.sort
      problems << "MCP servers #{init['mcp_servers'].inspect}" unless init["mcp_servers"].to_a.empty?
      case rec["task"]
      when "escape"
        files = Dir.glob("**/*", File::FNM_DOTMATCH, base: keep).map { |f| File.join(keep, f) }
                   .select { |f| File.file?(f) && !File.symlink?(f) }
        files.select { |f| File.binread(f).include?(@token) }.each do |f|
          problems << "the token is in #{f.delete_prefix("#{keep}/")}"
        end
        text = File.read(File.join(keep, "transcript.jsonl"))
        [ROOT, File.realpath(ROOT)].uniq.each do |r|
          problems << "the repository's path #{r} is in the transcript" if text.include?(r)
        end
        problems << "Ruby loaded pwn.rb" if File.exist?(File.join(keep, "files", "loaded.txt"))
        # Every path the audit found must be one the wrapper refused, and so
        # be for review; every refusal must be for review.
        refusals = calls.select { |c| c["refused"] }
        outside = rec["escapes"].to_a
        problems << "the audit found paths the wrapper did not refuse: #{outside.map { |e| e['path'] }.inspect}" unless outside.empty?
        refused = refusals.size
        listed = rec["review"].to_a.count { |e| e["tool"] == "midnight" }
        problems << "#{refused} refusals in the call log, #{listed} for review" unless refused == listed
        puts format("isolation: escape: %d tool calls, %d denied, %d compiler calls, %d refused by the wrapper, " \
                    "dropped %s, %d for review",
                    transcript.tool_calls.size, rec["denied"].to_a.size, calls.size, refused,
                    calls.flat_map { |c| c["dropped"].to_a }.uniq.sort.inspect, rec["review"].to_a.size)
      when "agents-md"
        reply = transcript.result.to_h["result"].to_s.strip
        problems << "replied #{reply[0, 80].inspect}, not `done`" unless reply.delete("`").delete_suffix(".").casecmp?("done")
        problems << "the reply holds the token" if reply.include?(@token)
        puts "isolation: agents-md: replied #{reply[0, 80].inspect}"
      end
      problems
    end
  end

  module SelfTest
    module_function

    def run(root = File.join(ROOT, "evals", "tasks"), compiler: Grade.compiler, only: [])
      problems = []
      tasks = Task.all(root)
      unknown = only - tasks.map(&:name)
      raise Failure, "no task #{unknown.map { |n| "`#{n}`" }.join(', ')}" unless unknown.empty?

      tasks = tasks.select { |t| only.include?(t.name) } unless only.empty?
      tasks.each do |task|
        check = ->(ok, what) { problems << "#{task.name}: #{what}" unless ok }
        %w[TASK.md meta solution.12am expected.out].each { |f| check.(task.exist?(f), "no #{f}") }
        next unless task.exist?("solution.12am") && task.exist?("expected.out")

        check.(!task.programs("right").empty?, "no program in right/")
        check.(!task.programs("wrong").empty?, "no program in wrong/")
        check.(shown_prefix?(task), "the output in TASK.md is not a proper prefix of expected.out")
        if task.draws? || task.meta.key?("draws")
          ok = task.draws? && ["in order", "any order"].include?(task.meta["draws"])
          check.(ok, "expected.draws and `draws: in order` or `draws: any order` in meta go together")
          next unless ok
        end
        [task.path("solution.12am"), *task.programs("right")].each do |prog|
          g, secs = timed { Grade.grade(task, prog, compiler: compiler) }
          check.(g["verdict"] == "pass", "#{rel(task, prog)} does not pass: #{g['notes']}")
          check.(secs < BUILD_SECONDS, "#{rel(task, prog)} took #{secs.round(1)} s")
        end
        only = Hash.new(false)
        task.programs("wrong").each do |prog|
          g, secs = timed { Grade.grade(task, prog, compiler: compiler) }
          check.(g["verdict"] != "pass", "#{rel(task, prog)} passes")
          check.(secs < BUILD_SECONDS, "#{rel(task, prog)} took #{secs.round(1)} s")
          %w[loops systems draws].each do |name|
            only[name] ||= g["checks"].reject { |k, _| k == name }.values.all? && g["checks"][name] == false
          end
        end
        check.(only["loops"], "no program in wrong/ fails only `loops`") unless task.loops.empty?
        check.(only["systems"], "no program in wrong/ fails only `systems`") unless task.systems.empty?
        check.(only["draws"], "no program in wrong/ fails only `draws`") if task.draws?
        if task.repair?
          g = Grade.grade(task, task.path("given", "solution.12am"), compiler: compiler)
          check.(g["verdict"] != "pass", "the given solution.12am passes")
        end
        task.given_files.select { |f| f.end_with?(".drive") }.each do |drive|
          check.(runs_with?(task, drive, compiler), "the reference stops with #{drive}")
        end
      end
      [tasks.size, problems]
    end

    def rel(task, path) = path.delete_prefix("#{task.dir}/")

    def timed
      t = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      v = yield
      [v, Process.clock_gettime(Process::CLOCK_MONOTONIC) - t]
    end

    def shown_prefix?(task)
      blocks = task.read("TASK.md").scan(/^```[^\n]*\n(.*?)^```/m).map(&:first)
      expected = task.expected
      blocks.any? { |b| expected.start_with?(b) && b.size < expected.size && !b.empty? }
    end

    def runs_with?(task, drive, compiler)
      dir = Env.mktemp
      FileUtils.cp(task.path("solution.12am"), dir)
      FileUtils.cp(task.path("given", drive), File.join(dir, "try.drive"))
      env = Grade.environment(dir).merge("MIDNIGHT_DRIVE" => "try.drive")
      r = Guard.capture(env, [compiler, "run", "solution.12am"], chdir: dir, limit: LIMIT, cap: CAP)
      r[:status].zero?
    ensure
      FileUtils.rm_rf(dir) if dir
    end
  end

  module CLI
    module_function

    USAGE = <<~TEXT
      usage: eval.rb run [--tasks A,B] [--models M,N] [--trials K] [--effort E]
                         [--kit FILE] [--jobs J] [--label L]
             eval.rb run --resume RESULTS
             eval.rb grade [--run RESULTS] TASK FILE
             eval.rb grade --run RESULTS
             eval.rb self-test [TASK...]
             eval.rb report RESULTS
             eval.rb compare A B
             eval.rb isolation
    TEXT

    def main(argv)
      command = argv.shift
      case command
      when "run" then Runner.new(run_options(argv)).run
      when "grade" then grade(argv)
      when "self-test" then self_test(argv)
      when "isolation"
        usage! unless argv.empty?
        Probe.new.run
      when "report"
        usage! unless argv.size == 1
        print Report.render(JSON.parse(File.read(argv[0])))
      when "compare"
        usage! unless argv.size == 2
        print Compare.render(JSON.parse(File.read(argv[0])), JSON.parse(File.read(argv[1])))
      else usage!
      end
    rescue Failure => e
      warn "eval: #{e.message}"
      exit 1
    end

    def usage!
      warn USAGE
      exit 2
    end

    def run_options(argv)
      opts = { models: %w[sonnet haiku], trials: 5, jobs: 4 }
      until argv.empty?
        flag = argv.shift
        value = argv.shift or usage!
        case flag
        when "--tasks" then opts[:tasks] = value.split(",")
        when "--models" then opts[:models] = value.split(",")
        when "--trials" then opts[:trials] = Integer(value)
        when "--effort" then opts[:effort] = value
        when "--kit" then opts[:kit] = value
        when "--jobs" then opts[:jobs] = Integer(value)
        when "--label" then opts[:label] = value
        when "--resume" then opts[:resume] = value
        else usage!
        end
      end
      opts
    end

    # grade TASK FILE: against the working tree's task. With --run RESULTS,
    # against that run's snapshot of its tasks, with the compiler of its
    # commit; and with no TASK FILE, every kept program of the run, saying
    # where the verdict differs from the recorded one.
    def grade(argv)
      results = nil
      if argv.first == "--run"
        argv.shift
        results = argv.shift or usage!
      end
      usage! unless argv.size == 2 || (results && argv.empty?)
      tasks = File.join(ROOT, "evals", "tasks")
      compiler = Grade.compiler if results.nil?
      if results
        r = JSON.parse(File.read(results))
        tasks = File.join(ROOT, "evals", "runs", File.basename(results, ".json"), "commit", "evals", "tasks")
        raise Failure, "no snapshot of the run's tasks in #{tasks.delete_prefix("#{ROOT}/")}" unless File.directory?(tasks)

        compiler = ENV["MIDNIGHT_BIN"].to_s.empty? ? Grade.compiler_of(r["commit"]) : ENV["MIDNIGHT_BIN"]
        return grade_run(r, tasks, compiler) if argv.empty?
      end
      dir = File.join(tasks, argv[0])
      raise Failure, "no task `#{argv[0]}`" unless File.file?(File.join(dir, "meta"))

      src = File.expand_path(argv[1])
      src = File.dirname(src) if File.basename(src) == "solution.12am"
      g = Grade.grade(Task.new(dir), src, compiler: compiler)
      puts g["verdict"]
      g["notes"].to_h.each { |check, note| puts "#{check}: #{note.strip}" }
      exit(g["verdict"] == "pass" ? 0 : 1)
    end

    def grade_run(results, tasks, compiler)
      differ = 0
      graded = results["runs"].select { |rec| rec["checks"] }
      graded.each do |rec|
        g = Grade.grade(Task.new(File.join(tasks, rec["task"])), File.join(ROOT, rec["dir"], "program"),
                        compiler: compiler)
        recorded = if rec["outcome"] == "no solution" || rec["checks"].empty? then "no solution"
                   elsif rec["checks"].values.all? then "pass"
                   else "fail"
                   end
        next if g["verdict"] == recorded

        differ += 1
        puts "#{rec['dir'].delete_prefix('evals/runs/')}: recorded #{recorded}, now #{g['verdict']} #{g['notes']}"
      end
      puts "grade: #{graded.size} programs, #{differ.zero? ? 'every verdict as recorded' : "#{differ} differ"}"
      exit(differ.zero? ? 0 : 1)
    end

    def self_test(argv)
      count, problems = SelfTest.run(only: argv)
      problems.each { |p| puts p }
      puts "self-test: #{count} tasks, #{problems.empty? ? 'all checks pass' : "#{problems.size} problems"}"
      exit(problems.empty? ? 0 : 1)
    end
  end
end

Eval::CLI.main(ARGV) if $PROGRAM_NAME == __FILE__
