# frozen_string_literal: true

# The evals' harness: runs agents on the tasks under evals/tasks, grades
# their programs, and writes results and reports. Standard library only.
#
#   ruby evals/eval.rb run [--tasks A,B] [--models M,N] [--trials K]
#                          [--effort E] [--kit FILE] [--jobs J] [--label L]
#   ruby evals/eval.rb run --resume RESULTS
#   ruby evals/eval.rb grade TASK FILE
#   ruby evals/eval.rb self-test
#   ruby evals/eval.rb report RESULTS
#   ruby evals/eval.rb compare A B

require "json"
require "digest"
require "fileutils"
require "shellwords"
require "time"
require "etc"
require "rbconfig"

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

      # The absolute path path names from dir, after `..` and symbolic links,
      # for as much of it as exists.
      def resolve(path, dir)
        full = File.expand_path(path, dir)
        rest = []
        probe = full
        until File.exist?(probe) || probe == "/"
          rest.unshift(File.basename(probe))
          probe = File.dirname(probe)
        end
        base = File.realpath(probe)
        rest.empty? ? base : File.join(base, *rest)
      end

      def inside?(path, dir)
        path == dir || path.start_with?(dir.end_with?("/") ? dir : "#{dir}/")
      end

      # The require paths of a source file that are absolute or contain `..`.
      def bad_requires(file)
        File.foreach(file).filter_map do |line|
          m = line.match(/\A\s*require\s+"((?:[^"\\]|\\.)*)"/)
          m[1] if m && (m[1].start_with?("/") || m[1].include?(".."))
        end
      rescue SystemCallError, ArgumentError
        []
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
          bad = bad_requires(file).first
          return "`#{File.basename(file)}` requires `#{bad}`, outside this directory" if bad
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
          "TMPDIR" => "/tmp",
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
          "passed" => env.reject { |k, _| %w[PATH HOME XDG_RUNTIME_DIR].include?(k) },
          "dropped" => dropped,
          "caller" => caller_env.select { |k, _| k.start_with?("MIDNIGHT_") },
          "load" => load_average
        }
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
    def control? = @meta["control"] == "yes"
    def areas = @meta.fetch("areas", "").split
    def expected = read("expected.out")
    def drive = exist?("check.drive") ? path("check.drive") : nil
    def repair? = given_files.include?("solution.12am")

    def given_files
      return [] unless exist?("given")

      Dir.glob("**/*", base: path("given")).select { |f| File.file?(path("given", f)) }.sort
    end

    def programs(kind)
      Dir.glob("*.12am", base: path(kind)).sort.map { |f| path(kind, f) }
    end

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
      %w[TASK.md expected.out check.drive loops].each do |f|
        d << "#{f}\0#{exist?(f) ? read(f) : ''}\0"
      end
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

    # Each `##` and `###` heading of a Markdown file as [line, title]; a
    # file with none gets one section for the whole file.
    def sections(text)
      fence = false
      found = []
      text.each_line.with_index(1) do |line, n|
        fence = !fence if line.start_with?("```")
        next if fence

        m = line.match(/\A(##|###) (.+)/)
        found << [n, m[2].strip] if m
      end
      found.empty? ? [[1, nil]] : found
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

    # Each diagnostic, run-time error and `midnight:` line.
    def parse(text)
      lines = text.to_s.lines.map(&:chomp)
      out = []
      lines.each_with_index do |line, i|
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

    # Whether a run of solution.12am exited 0 with the graded content.
    def verified(calls, graded_sha)
      return false unless graded_sha

      calls.any? do |c|
        !c["refused"] && subcommand(c) == "run" && c["file"] == "solution.12am" &&
          c["sha256"] == graded_sha && c["status"].zero?
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

    # Why the init line shows the flags no longer isolate, or nil.
    def self.isolation_problem(init)
      return "no init line" unless init

      problems = []
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
    def infrastructure
      r = result
      return ["no result line", false] unless r
      return nil unless r["is_error"]

      text = r["result"].to_s
      return ["usage limit: #{text[0, 200]}", true] if text.match?(USAGE) || r["api_error_status"] == 429
      return ["API error: #{text[0, 200]}", false] if text.match?(API) || r["api_error_status"]

      nil
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

    # The words of a shell command that may be paths. After `cd DIR`, the
    # relative words of later commands are taken from DIR.
    def bash_words(command)
      dir = nil
      segments(command).flat_map do |seg|
        words = begin
          Shellwords.split(seg)
        rescue ArgumentError
          seg.split(/\s+/)
        end
        found = segment_words(words)
        found = found.map { |w| w.start_with?("/", "~") ? w : File.join(dir, w) } if dir
        if words.first == "cd" && words[1]
          dir = words[1].start_with?("/", "~") || dir.nil? ? words[1] : File.join(dir, words[1])
        end
        found
      end
    end

    def path_like?(word) = word.start_with?("/", "~") || word.include?("..")

    # The path-like words of one simple command: assignments' values, the
    # command, redirection targets and arguments, but not the pattern or
    # script of grep, rg, sed and awk, nor what echo and printf print.
    def segment_words(seg)
      seg = seg.dup
      found = []
      while seg.size > 1 && (m = seg.first.match(/\A[A-Za-z_][A-Za-z_0-9]*=(.*)\z/m))
        found << m[1]
        seg.shift
      end
      return found.select { |w| path_like?(w) } if seg.empty?

      found << seg.first
      cmd = File.basename(seg.first)
      pattern_due = PATTERN_FIRST.include?(cmd)
      skip_next = false
      seg.drop(1).each do |w|
        if skip_next
          skip_next = false
        elsif (m = w.match(REDIRECT))
          target = w.delete_prefix(m[0])
          found << target unless target.empty? || target.start_with?("&")
        elsif NO_PATHS.include?(cmd)
          next
        elsif PATTERN_FIRST.include?(cmd) && %w[-e --regexp --expression].include?(w)
          skip_next = true
          pattern_due = false
        elsif w.start_with?("-")
          next
        elsif pattern_due
          pattern_due = false
        else
          found << w
        end
      end
      found.select { |w| path_like?(w) }
    end

    # The candidate paths of one tool call.
    def paths(call)
      input = call["input"]
      case call["tool"]
      when "Bash" then bash_words(input["command"].to_s)
      when "Read", "Edit", "Write" then [input["file_path"]].compact
      when "Glob" then [input["path"], glob_base(input["pattern"])].compact
      when "Grep" then [input["path"], glob_base(input["glob"])].compact
      else []
      end
    end

    def glob_base(pattern)
      return nil unless pattern

      i = pattern.index(/[*?\[{]/)
      return pattern unless i

      cut = pattern[0, i].rindex("/")
      cut ? pattern[0, cut + 1] : nil
    end

    def expand(path, root)
      path = File.join(Dir.home, path.delete_prefix("~")) if path == "~" || path.start_with?("~/")
      File.expand_path(path, root)
    end

    # The paths, outside the allowed places, that calls which ran reached.
    def escapes(calls, root:, wrapper:, tool_results:)
      allowed = [root, wrapper, tool_results].compact
      calls.reject { |c| c["denied"] }.flat_map do |c|
        paths(c).filter_map do |p|
          full = expand(p, root)
          next if %w[/dev/null /dev/stdout /dev/stderr].include?(full)
          next if allowed.any? { |a| Guard.inside?(full, a) }

          { "tool" => c["tool"], "path" => p }
        end
      end
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

    # An environment like the wrapper's for a directory of its own.
    def environment(dir)
      home = File.join(dir, ".home")
      xdg = File.join(dir, ".xdg")
      FileUtils.mkdir_p([home, xdg])
      Guard.environment({}, { "path" => Env.coreutils, "home" => home, "xdg" => xdg }).first
    end

    # Copies every .12am file under src (or src itself, as solution.12am)
    # into a fresh directory with the task's check.drive, and grades it.
    # Returns { "verdict", "checks" => {build:, output:, loops:}, ... }.
    def grade(task, src, compiler: Grade.compiler, keep: nil)
      LOCK.synchronize do
        dir = Env.mktemp
        begin
          if File.directory?(src)
            Dir.glob("**/*.12am", base: src).each do |f|
              FileUtils.mkdir_p(File.dirname(File.join(dir, f)))
              FileUtils.cp(File.join(src, f), File.join(dir, f))
            end
          else
            FileUtils.cp(src, File.join(dir, "solution.12am"))
          end
          FileUtils.cp(task.drive, File.join(dir, "check.drive")) if task.drive
          result = grade_dir(task, dir, compiler)
          stdout = result.delete("stdout")
          if keep
            FileUtils.mkdir_p(keep)
            File.write(File.join(keep, "graded.out"), stdout)
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
        return { "verdict" => "no solution", "checks" => {}, "sha256" => nil, "stdout" => "" }
      end

      env = environment(dir)
      checks = {}
      notes = {}
      sha = Guard.sha256(solution)
      refused = Guard.refusal(["build", "solution.12am"], {}, dir, File.realpath(dir))
      if refused
        notes["build"] = refused
        checks["build"] = false
      else
        b = Guard.capture(env, [compiler, "build", "solution.12am", "-o", "solution"],
                          chdir: dir, limit: LIMIT, cap: CAP)
        checks["build"] = b[:status].zero?
        notes["build"] = b[:err].lines.first(5).join unless checks["build"]
      end
      stdout = ""
      if checks["build"]
        run_env = task.drive ? env.merge("MIDNIGHT_DRIVE" => "check.drive") : env
        r = Guard.capture(run_env, [File.join(dir, "solution")], chdir: dir, limit: LIMIT, cap: CAP)
        stdout = r[:out].scrub
        checks["output"] = r[:status].zero? && r[:out] == task.expected
        unless checks["output"]
          notes["output"] = r[:timeout] ? "time limit" : "status #{r[:status]}"
          notes["output"] += ", #{first_difference(r[:out], task.expected)}" if r[:status].zero?
        end
      else
        checks["output"] = false
      end
      unless task.loops.empty?
        l = Guard.capture(env, [compiler, "loops", "solution.12am"], chdir: dir, limit: LIMIT, cap: CAP)
        checks["loops"] = l[:status].zero? && loops_ok?(l[:out], task.loops)
        notes["loops"] = l[:out].lines.first(10).join unless checks["loops"]
      end
      verdict = checks.values.all? ? "pass" : "fail"
      { "verdict" => verdict, "checks" => checks, "notes" => notes, "sha256" => sha, "stdout" => stdout }
    end

    def first_difference(got, want)
      g = got.lines
      w = want.lines
      i = (0...[g.size, w.size].max).find { |k| g[k] != w[k] }
      "line #{i + 1}: got #{g[i].to_s.chomp.inspect}, want #{w[i].to_s.chomp.inspect}"
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
      end
      Eval.write_wrapper(@wrapper, root: @root, logs: @logs, compiler: @runner.compiler)
    end

    def command
      cmd = [@runner.claude, "-p", "--safe-mode", "--restricted", "--strict-mcp-config",
             "--disable-slash-commands", "--tools", TOOLS.join(","),
             "--allowedTools", "Bash(midnight *)", "Bash(MIDNIGHT_* midnight *)", *TOOLS.drop(1),
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
            problem = Transcript.isolation_problem(Transcript.new(first).init)
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
        @isolation = Transcript.isolation_problem(Transcript.load(transcript).init)
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
      program_dir = File.join(@keep, "program")
      Dir.glob("**/*.12am", base: @root).reject { |f| f.start_with?("docs/") }.each do |f|
        FileUtils.mkdir_p(File.dirname(File.join(program_dir, f)))
        FileUtils.cp(File.join(@root, f), File.join(program_dir, f))
      end
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

      escapes = Audit.escapes(tool_calls, root: @root, wrapper: @wrapper,
                                          tool_results: Eval.tool_results_dir(@root))
      escapes += calls.select { |c| c["refused"] }.map { |c| { "tool" => "midnight", "path" => c["refused"] } }
      graded = Grade.grade(@task, @root, compiler: @runner.compiler, keep: @keep)
      given_sha = @task.repair? ? Guard.sha256(@task.path("given", "solution.12am")) : nil
      rec["outcome"] =
        if !escapes.empty? then "escaped"
        elsif status == :timeout then "timeout"
        elsif graded["verdict"] == "no solution" then "no solution"
        else graded["verdict"]
        end
      rec["checks"] = graded["checks"]
      rec["failed_checks"] = graded["checks"].reject { |_, v| v }.keys
      rec["grade_notes"] = graded["notes"] unless graded["notes"].to_h.empty?
      rec["graded_sha256"] = graded["sha256"]
      rec["escapes"] = escapes unless escapes.empty?
      rec["calls"] = CallLog.summary(calls)
      rec["diagnostics"] = CallLog.diagnostics(calls)
      rec["compiler_bug"] = calls.any? { |c| Stderr.compiler_bug?(c["stderr"]) }
      rec["first_try"] = CallLog.first_try(calls, given_sha)
      rec["verified"] = CallLog.verified(calls, graded["sha256"])
      rec["denied"] = tool_calls.select { |c| c["denied"] }.map do |c|
        { "tool" => c["tool"], "input" => c["input"]["command"] || c["input"]["file_path"] || c["input"]["path"] }
      end
      rec["slow_calls"] = calls.count { |c| c["timeout"] && c["load"].to_f > Etc.nprocessors }
      rec.merge(transcript.usage)
    end

    def cleanup
      [@root, @wrapper, @logs].compact.each { |d| FileUtils.rm_rf(d) }
      dir = @root && Eval.tool_results_dir(@root)
      Dir.rmdir(dir) if dir && File.directory?(dir) && Dir.empty?(dir)
    rescue SystemCallError
      nil
    end
  end

  # Claude Code's directory for a working directory's spilled tool results.
  def self.tool_results_dir(root)
    File.join(Dir.home, ".claude", "projects", root.gsub(/[^A-Za-z0-9]/, "-"))
  end

  # Writes the agent's `midnight` into dir.
  def self.write_wrapper(dir, root:, logs:, compiler:, limit: LIMIT, cap: CAP)
    home = File.join(logs, "home")
    xdg = File.join(logs, "xdg")
    FileUtils.mkdir_p([home, xdg])
    config = { "root" => File.realpath(root), "log" => File.join(logs, "calls.jsonl"),
               "compiler" => compiler, "path" => Env.coreutils, "home" => home, "xdg" => xdg,
               "limit" => limit, "cap" => cap }
    path = File.join(dir, "midnight")
    File.write(path, <<~SCRIPT)
      #!#{Env.ruby} --disable-gems
      # frozen_string_literal: true
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

    def stopping? = @mutex.synchronize { @stopping }
    def started(pid) = @mutex.synchronize { @pids << pid }
    def finished(pid) = @mutex.synchronize { @pids.delete(pid) }

    def setup
      @commit = Git.head
      raise Failure, "evals/eval.rb differs from #{@commit[0, 7]}; commit it first" unless Git.clean?(@commit, "evals/eval.rb")

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
      @results_path ||= File.join(ROOT, "evals", "results", "#{name}.json")
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

      Git.export(@commit, "evals/tasks", snapshot)
      Git.export(@commit, "evals/preamble.md", snapshot)
      entries = Kit.parse(Git.show(@commit, kit_file))
      kit = Kit.files(entries, Git.files(@commit))
      kit.each_value { |source| Git.export(@commit, source, snapshot) }
      @kit_files = kit.transform_values { |s| File.join(snapshot, s) }
      @preamble = File.read(File.join(snapshot, "evals", "preamble.md"))
      all = Task.all(File.join(snapshot, "evals", "tasks"))
      names = @opts[:tasks] || all.map(&:name)
      unknown = names - all.map(&:name)
      raise Failure, "no task #{unknown.join(', ')}" unless unknown.empty?

      @tasks = all.select { |t| names.include?(t.name) }
      @compiler = build_compiler
      @results ||= header(kit_file, kit)
    end

    def build_compiler
      out = IO.popen(["nix", "build", "git+file://#{ROOT}?rev=#{@commit}", "--no-link", "--print-out-paths"], &:read)
      raise Failure, "nix build of #{@commit[0, 7]} failed" unless $?.success?

      File.join(out.lines.last.strip, "bin", "midnight")
    end

    def header(kit_file, kit)
      {
        "format" => 1, "commit" => @commit,
        "version" => IO.popen([@compiler, "--version"], &:read).to_s.split.last,
        "ruby" => RUBY_VERSION, "claude_code" => @claude_version, "date" => @date,
        "label" => @opts[:label], "models" => @opts[:models], "trials" => @opts[:trials],
        "effort" => @opts[:effort], "task_names" => @tasks.map(&:name),
        "kit" => {
          "file" => kit_file,
          "files" => kit.map { |dest, source| { "dest" => dest, "source" => source, "sha256" => Digest::SHA256.file(@kit_files[dest]).hexdigest } },
          "sections" => kit.keys.to_h { |dest| [dest, dest.end_with?(".md") ? Kit.sections(File.read(@kit_files[dest])) : [[1, nil]]] }
        },
        "tasks" => @tasks.to_h { |t| [t.name, { "sha256" => t.sha256, "control" => t.control?, "areas" => t.areas, "ruby" => t.meta["ruby"].to_s }] },
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
            next if mine.any? { |r| r["outcome"] != "infrastructure" }
            next if mine.count { |r| r["counts"] } >= 3

            [t, m, k, mine.size + 1]
          end
        end
      end
    end

    def save
      @mutex.synchronize do
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
      File.write(report, Report.render(@results)) if defined?(Report)
      puts "eval: results in #{rel}"
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
  module SelfTest
    module_function

    def run(root = File.join(ROOT, "evals", "tasks"), compiler: Grade.compiler)
      problems = []
      tasks = Task.all(root)
      tasks.each do |task|
        check = ->(ok, what) { problems << "#{task.name}: #{what}" unless ok }
        %w[TASK.md meta solution.12am expected.out].each { |f| check.(task.exist?(f), "no #{f}") }
        next unless task.exist?("solution.12am") && task.exist?("expected.out")

        check.(!task.programs("right").empty?, "no program in right/")
        check.(!task.programs("wrong").empty?, "no program in wrong/")
        check.(shown_prefix?(task), "the output in TASK.md is not a proper prefix of expected.out")
        check.(task.control? ? task.meta["ruby"].to_s.empty? : !task.meta["ruby"].to_s.empty?,
               task.control? ? "a control names Ruby forms" : "names no Ruby form")
        [task.path("solution.12am"), *task.programs("right")].each do |prog|
          g, secs = timed { Grade.grade(task, prog, compiler: compiler) }
          check.(g["verdict"] == "pass", "#{rel(task, prog)} does not pass: #{g['notes']}")
          check.(secs < BUILD_SECONDS, "#{rel(task, prog)} took #{secs.round(1)} s")
        end
        only_loops = false
        task.programs("wrong").each do |prog|
          g, secs = timed { Grade.grade(task, prog, compiler: compiler) }
          check.(g["verdict"] != "pass", "#{rel(task, prog)} passes")
          check.(secs < BUILD_SECONDS, "#{rel(task, prog)} took #{secs.round(1)} s")
          only_loops ||= g["checks"]["build"] && g["checks"]["output"] && g["checks"]["loops"] == false
        end
        check.(only_loops, "no program in wrong/ fails only `loops`") unless task.loops.empty?
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
             eval.rb grade TASK FILE
             eval.rb self-test
             eval.rb report RESULTS
             eval.rb compare A B
    TEXT

    def main(argv)
      command = argv.shift
      case command
      when "run" then Runner.new(run_options(argv)).run
      when "grade" then grade(argv)
      when "self-test" then self_test(argv)
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

    def grade(argv)
      usage! unless argv.size == 2
      task = Task.new(File.join(ROOT, "evals", "tasks", argv[0]))
      src = File.expand_path(argv[1])
      src = File.dirname(src) if File.basename(src) == "solution.12am"
      g = Grade.grade(task, src)
      puts g["verdict"]
      g["notes"].to_h.each { |check, note| puts "#{check}: #{note.strip}" }
      exit(g["verdict"] == "pass" ? 0 : 1)
    end

    def self_test(argv)
      usage! unless argv.empty?
      count, problems = SelfTest.run
      problems.each { |p| puts p }
      puts "self-test: #{count} tasks, #{problems.empty? ? 'all checks pass' : "#{problems.size} problems"}"
      exit(problems.empty? ? 0 : 1)
    end
  end
end

Eval::CLI.main(ARGV) if $PROGRAM_NAME == __FILE__
