# frozen_string_literal: true

require_relative 'test_helper'
require_relative 'terminal_session'

class HdfTestCase < Minitest::Test
  CRATE = ROOT
  HDF_DIR = File.join(CRATE, 'target', 'debug')
  HDF = File.join(HDF_DIR, 'hdf')
  HDFRC = File.join(ROOT, 'home', '.hdfrc')

  HISTORY = [
    'cd src/sndbx/',
    'git fetch --all --prune',
    'sbx run claude ',
    'tmux -CC',
    'sbx -V',
    'sbx run claude ',
    'gg',
    'ga .',
    "git cm 'Align the recalculation status'",
    'sbx build claude ',
    'sbx run claude',
    'tc',
    'sbx run codex ',
    './install.rb ',
    'claude --resume',
    'git push ',
    'Claude',
    'gg',
  ].map { |line| "#{line}\n" }.join.freeze

  system('cargo', 'build', '--quiet', '--manifest-path', File.join(CRATE, 'Cargo.toml')) || abort('cargo build failed')

  def setup
    @tmp_dir = Dir.mktmpdir
    @history = File.join(@tmp_dir, 'bash_history')
    File.write(@history, HISTORY)
  end

  def teardown
    @session&.close
    FileUtils.remove_entry(@tmp_dir)
  end

  DIVIDER_ROW = 1
  FIRST_ROW = 2
  PANE_DIVIDER = %r{\A─* ?(\d+/\d+|deleted .*|cannot .*) ──\z}

  private

  def start(*query, command: [HDF, *query], history: @history, env: {}, **terminal)
    env = { 'HISTFILE' => history, 'TZ' => 'UTC', 'XDG_CONFIG_HOME' => config_home }.merge(env)
    @session = TerminalSession.new(command, env:, **terminal)
    wait_for_pane
  end

  def bash_with_hdfrc(commands)
    ['bash', '--norc', '--noprofile', '-i', '-c', ". #{HDFRC}; #{commands}"]
  end

  def start_bash(path: HDF_DIR, ps1: '$ ')
    rc = File.join(@tmp_dir, 'bashrc')
    File.write(rc, <<~BASH)
      PS1='#{ps1}'
      HISTFILE=#{@history}
      PROMPT_COMMAND='history -a; history -c; history -r'
      . #{HDFRC}
    BASH
    env = {
      'PATH' => "#{path}:#{ENV.fetch('PATH')}", 'HOME' => @tmp_dir, 'XDG_CONFIG_HOME' => nil, 'INPUTRC' => File::NULL,
      'TZ' => 'UTC',
    }
    @session = TerminalSession.new(['bash', '--noprofile', '--rcfile', rc, '-i'], env:)
    @session.wait_for { |lines| !lines.first.empty? }
  end

  def wait_for_hdf_in_bash(query = '')
    prompt = "$ #{query}".rstrip
    @session.wait_for { |lines| lines.each_cons(2).any? { |line, below| line == prompt && below.match?(PANE_DIVIDER) } }
    sleep(0.05)
  end

  def wait_for_pane
    @session.wait_for { |lines| lines.any? { |line| line.match?(PANE_DIVIDER) } }
    sleep(0.05)
  end

  def pane
    lines = @session.screen
    divider = lines.index { |line| line.match?(PANE_DIVIDER) }
    flunk("no pane under a prompt line:\n#{lines.join("\n")}") unless divider&.positive?

    [lines[divider - 1], *lines[(divider + 1)..].take_while { |line| !line.empty? }.map(&:strip)]
  end

  def picked_output(*keys)
    output = File.join(@tmp_dir, 'output')
    start(command: ['sh', '-c', %("$0" > "#{output}"), HDF])
    @session.press(*keys)
    @session.wait
    @session = nil

    File.read(output)
  end

  def assert_history(expected)
    @session.wait_for { |lines| lines.grep(/\A +\d/).size >= expected.size }

    assert_equal expected, @session.screen.grep(/\A +\d/)
  end

  def delete
    @session.press(:ctrl_d)
    @session.wait_for { |lines| lines.any? { |line| line.match?(/ (deleted|cannot) /) } }
    sleep(0.05)
  end

  def assert_picks(*keys, command, status: 0)
    @session.press(*keys)

    assert_equal status, @session.wait.exitstatus, @session.screen.join("\n")
    assert_equal command, @session.screen.join("\n").strip
  end

  def assert_cancels(*keys, status: 1, output: '')
    @session.press(*keys)

    assert_equal status, @session.wait.exitstatus
    assert_equal output, @session.screen.join("\n").strip
  end

  def run_hdf(*, env: {})
    Open3.capture3({ 'HISTFILE' => @history, 'XDG_CONFIG_HOME' => config_home }.merge(env), HDF, *)
  end

  # Runs hdf with stdout on a pipe nobody reads, so every write fails with EPIPE
  def run_hdf_into_a_closed_pipe(*)
    out_reader, out_writer = IO.pipe
    out_reader.close
    err_reader, err_writer = IO.pipe
    pid = Process.spawn({ 'HISTFILE' => @history, 'XDG_CONFIG_HOME' => config_home }, HDF, *,
                        out: out_writer, err: err_writer)
    [out_writer, err_writer].each(&:close)
    stderr = err_reader.read
    err_reader.close

    [stderr, Process.wait2(pid).last]
  end

  def assert_color(code)
    assert_match(color_pattern(code), @session.output)
  end

  def refute_color(code)
    refute_match(color_pattern(code), @session.output)
  end

  def color_pattern(code)
    /\e\[(?:[\d;]*;)?#{Regexp.escape(code)}[;m]/
  end

  def config_home
    File.join(@tmp_dir, 'config')
  end

  def write_config(text, home: config_home)
    FileUtils.mkdir_p(File.join(home, 'hdf'))
    File.write(File.join(home, 'hdf', 'config.toml'), text)
  end
end
